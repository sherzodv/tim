use std::collections::VecDeque;
use std::sync::Arc;

use async_trait::async_trait;
use serde::Serialize;
use tokio::time::{sleep, Duration};
use tracing::debug;

use crate::agent::{Agent, AgentBuilder, AgentError};
use crate::prompt::render as render_template;
use crate::tim_client::tim_api::{Ability as SpaceAbility, AbilityParameter, TimiteAbilities};
use crate::tim_client::TimClient;

use super::chatgpt::ChatGpt;
use super::llm::{Llm, LlmReq};

#[derive(Clone)]
pub struct LlmAgentConf {
    pub initial_msg: Option<String>,
    pub userp: String,
    pub history_limit: usize,
    pub response_delay: Duration,
    pub api_key: String,
    pub endpoint: String,
    pub model: String,
    pub temperature: f32,
}

pub struct LlmAgent {
    client: TimClient,
    conf: LlmAgentConf,
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

const SPACE_ABILITIES_TEMPLATE: &str = include_str!("../../prompts/space_abilities.txt");
const SPACE_ABILITY_ENTRY_TEMPLATE: &str = include_str!("../../prompts/space_ability_entry.txt");
const TIM_SYSTEM_PROMPT: &str = include_str!("../../prompts/tim-sys.md");

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
    pub fn new(conf: &LlmAgentConf, client: TimClient) -> Result<Self, AgentError> {
        let llm: Arc<dyn Llm> = Arc::new(
            ChatGpt::new(
                conf.api_key.clone(),
                conf.endpoint.clone(),
                conf.model.clone(),
                conf.temperature,
            )
            .map_err(|err| AgentError::Llm(err.to_string()))?,
        );
        Ok(Self {
            client,
            conf: conf.clone(),
            llm,
            history: VecDeque::with_capacity(conf.history_limit),
        })
    }

    async fn respond(&self, prompt: &str) -> Result<String, AgentError> {
        let req = LlmReq {
            sysp: TIM_SYSTEM_PROMPT,
            userp: &self.conf.userp,
            msg: prompt,
        };
        debug!(
            target: "tim_agent::llm",
            prompt = prompt,
            "Dispatching LLM chat request"
        );
        let answer = self
            .llm
            .chat(&req)
            .await
            .map_err(|err| AgentError::Llm(err.to_string()))?;
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

impl AgentBuilder for LlmAgentConf {
    type A = LlmAgent;

    fn build(&self, client: TimClient) -> Result<Self::A, AgentError> {
        LlmAgent::new(self, client)
    }
}
