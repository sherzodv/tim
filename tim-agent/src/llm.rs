pub mod chatgpt;

pub use chatgpt::{OPENAI_DEFAULT_ENDPOINT, OPENAI_DEFAULT_MODEL};

use async_trait::async_trait;
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tracing::debug;

use self::chatgpt::ChatGpt;
use crate::agent::{Agent, AgentBuilder, AgentError};
use crate::prompt::render as render_template;
use crate::tim_client::tim_api::{Ability as SpaceAbility, AbilityParameter, TimiteAbilities};
use crate::tim_client::TimClient;

pub struct LlmReq<'a> {
    pub sysp: &'a str,
    pub userp: &'a str,
    pub msg: &'a str,
}

pub struct LlmRes {
    pub message: String,
}

#[derive(Clone)]
pub struct LlmConf {
    pub initial_msg: Option<String>,
    pub sysp: String,
    pub userp: String,
    pub history_limit: usize,
    pub response_delay: Duration,
    pub api_key: String,
    pub endpoint: String,
    pub model: String,
    pub temperature: f32,
}

#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("llm prompt is empty")]
    EmptyPrompt,
    #[error("missing OpenAI API key (set OPENAI_API_KEY)")]
    MissingApiKey,
    #[error("http error while contacting LLM: {0}")]
    Http(#[from] reqwest::Error),
    #[error("failed to decode LLM response: {0}")]
    Response(#[from] serde_json::Error),
    #[error("LLM reported an error: {0}")]
    Api(String),
    #[error("LLM response missing message content")]
    MissingContent,
}

#[async_trait]
pub trait Llm: Send + Sync {
    async fn chat(&self, req: &LlmReq<'_>) -> Result<LlmRes, LlmError>;
}

pub struct LlmAgent {
    client: TimClient,
    conf: LlmConf,
    llm: Arc<dyn Llm>,
    history: VecDeque<DialogTurn>,
}

#[derive(Clone, Copy)]
enum DialogRole {
    Peer,
    Agent,
}

struct DialogTurn {
    role: DialogRole,
    content: String,
}

const SPACE_ABILITIES_TEMPLATE: &str = include_str!("../prompts/space_abilities.txt");
const SPACE_ABILITY_ENTRY_TEMPLATE: &str = include_str!("../prompts/space_ability_entry.txt");

#[derive(Serialize)]
struct AbilityEntryTemplateCtx {
    owner: String,
    name: String,
    description: String,
    params: String,
}

#[derive(Serialize)]
struct SpaceAbilitiesTemplateCtx<'a> {
    entries: &'a str,
}

impl LlmAgent {
    pub fn new(conf: &LlmConf, client: TimClient) -> Result<Self, AgentError> {
        let llm: Arc<dyn Llm> = Arc::new(ChatGpt::new(
            conf.api_key.clone(),
            conf.endpoint.clone(),
            conf.model.clone(),
            conf.temperature,
        )?);
        Ok(Self {
            client,
            conf: conf.clone(),
            llm,
            history: VecDeque::with_capacity(conf.history_limit),
        })
    }

    async fn respond(&self, prompt: &str) -> Result<String, AgentError> {
        let req = LlmReq {
            sysp: &self.conf.sysp,
            userp: &self.conf.userp,
            msg: prompt,
        };
        debug!(
            target: "tim_agent::llm",
            prompt = prompt,
            "Dispatching LLM chat request"
        );
        let answer = self.llm.chat(&req).await?;
        debug!(
            target: "tim_agent::llm",
            response = answer.message.as_str(),
            "Received LLM chat response"
        );
        Ok(answer.message)
    }

    fn push_history(&mut self, role: DialogRole, content: &str) {
        let trimmed = content.trim();
        if trimmed.is_empty() {
            return;
        }
        if self.history.len() == self.conf.history_limit {
            self.history.pop_front();
        }
        self.history.push_back(DialogTurn {
            role,
            content: trimmed.to_string(),
        });
    }

    fn render_history(&self) -> String {
        if self.history.is_empty() {
            return String::new();
        }
        let mut buf = String::new();
        for turn in &self.history {
            let role = match turn.role {
                DialogRole::Peer => "Peer",
                DialogRole::Agent => "Agent",
            };
            buf.push_str(role);
            buf.push_str(": ");
            buf.push_str(&turn.content);
            buf.push('\n');
        }
        buf.trim_end().to_string()
    }

    async fn render_space_abilities(&mut self) -> Result<Option<String>, AgentError> {
        let abilities = self.client.list_abilities().await?;
        let mut entries = Vec::new();
        for envelope in &abilities {
            let owner = Self::ability_owner(envelope);
            for ability in &envelope.abilities {
                if let Some(ctx) = Self::ability_entry_ctx(&owner, ability) {
                    entries.push(render_template(SPACE_ABILITY_ENTRY_TEMPLATE, &ctx)?);
                }
            }
        }
        if entries.is_empty() {
            return Ok(None);
        }
        let block = entries.join("\n");
        let ctx = SpaceAbilitiesTemplateCtx {
            entries: block.trim(),
        };
        let rendered = render_template(SPACE_ABILITIES_TEMPLATE, &ctx)?;
        Ok(Some(rendered))
    }

    fn ability_owner(envelope: &TimiteAbilities) -> String {
        envelope
            .timite
            .as_ref()
            .map(|timite| {
                let nick = timite.nick.trim();
                if nick.is_empty() {
                    format!("timite#{}", timite.id)
                } else {
                    nick.to_string()
                }
            })
            .unwrap_or_else(|| "unknown".to_string())
    }

    fn ability_entry_ctx(owner: &str, ability: &SpaceAbility) -> Option<AbilityEntryTemplateCtx> {
        let name = ability.name.trim();
        if name.is_empty() {
            return None;
        }
        let description = ability.description.trim();
        Some(AbilityEntryTemplateCtx {
            owner: owner.to_string(),
            name: name.to_string(),
            description: if description.is_empty() {
                "no description provided".to_string()
            } else {
                description.to_string()
            },
            params: Self::format_params(&ability.params),
        })
    }

    fn format_params(params: &[AbilityParameter]) -> String {
        if params.is_empty() {
            return "none".to_string();
        }
        params
            .iter()
            .map(|param| {
                let name = param.name.trim();
                let desc = param.description.trim();
                match (name.is_empty(), desc.is_empty()) {
                    (true, true) => "value".to_string(),
                    (true, false) => desc.to_string(),
                    (false, true) => name.to_string(),
                    (false, false) => format!("{name} ({desc})"),
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[async_trait]
impl Agent for LlmAgent {
    async fn on_start(&mut self) -> Result<(), AgentError> {
        if let Some(initial) = self.conf.initial_msg.take() {
            self.client.send_message(&initial).await?;
        }
        if let Some(abilities) = self.render_space_abilities().await? {
            debug!(
                target: "tim_agent::llm",
                abilities = abilities.as_str(),
                "Fetched space abilities"
            );
        }
        Ok(())
    }

    async fn on_space_message(&mut self, _sender_id: u64, content: &str) -> Result<(), AgentError> {
        if !self.conf.response_delay.is_zero() {
            sleep(self.conf.response_delay).await;
        }
        self.push_history(DialogRole::Peer, content);
        let context = self.render_history();
        let prompt_body = if context.is_empty() {
            content.trim().to_string()
        } else {
            format!("Conversation so far:\n{context}\nRespond to the latest peer message.")
        };
        let reply = self.respond(&prompt_body).await?;
        self.push_history(DialogRole::Agent, &reply);
        self.client.send_message(&reply).await?;
        Ok(())
    }
}

impl AgentBuilder for LlmConf {
    type A = LlmAgent;

    fn build(&self, client: TimClient) -> Result<Self::A, AgentError> {
        LlmAgent::new(self, client)
    }
}
