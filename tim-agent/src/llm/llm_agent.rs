use std::sync::Arc;

use async_trait::async_trait;
use serde::Serialize;
use tokio::time::{sleep, Duration};
use tracing::debug;

use crate::agent::{Agent, AgentBuilder, AgentError};
use crate::prompt::render as render_template;
use crate::tim_client::tim_api::{Ability as SpaceAbility, AbilityParameter, TimiteAbilities};
use crate::tim_client::TimClient;
use crate::tim_client::{Event, SpaceNewMessage, SpaceUpdate};

use super::chatgpt::ChatGpt;
use super::llm::{Llm, LlmReq};
use super::memory::LlmMemory;

#[derive(Clone)]
pub struct LlmAgentConf {
    pub userp: String,
    pub history_limit: usize,
    pub response_delay: Duration,
    pub api_key: String,
    pub endpoint: String,
    pub model: String,
    pub temperature: f32,
    pub live_interval: Duration,
}

pub struct LlmAgent {
    client: TimClient,
    conf: LlmAgentConf,
    llm: Arc<dyn Llm>,
    memory: LlmMemory,
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
            memory: LlmMemory::new(conf.history_limit),
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

    async fn reply_with_prompt(&mut self, prompt_body: String) -> Result<(), AgentError> {
        let reply = self.respond(&prompt_body).await?;
        let _ = self.memory.push_agent(&reply);
        self.client.send_message(&reply).await?;
        Ok(())
    }

    async fn handle_peer_message(&mut self, content: String) -> Result<(), AgentError> {
        if !self.conf.response_delay.is_zero() {
            sleep(self.conf.response_delay).await;
        }
        let _ = self.memory.push_peer(&content);
        let prompt_body = match self.memory.context() {
            Some(context) => {
                format!("Conversation so far:\n{context}\nRespond to the latest peer message.")
            }
            None => content.trim().to_string(),
        };
        self.reply_with_prompt(prompt_body).await
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
        if let Some(abilities) = self.render_space_abilities().await? {
            debug!(
                target: "tim_agent::llm",
                abilities = abilities.as_str(),
                "Fetched space abilities"
            );
        }
        Ok(())
    }

    async fn on_space_update(&mut self, update: &SpaceUpdate) -> Result<(), AgentError> {
        match &update.event {
            Some(Event::SpaceNewMessage(SpaceNewMessage {
                message: Some(message),
            })) => {
                let content = message.content.clone();
                self.handle_peer_message(content).await
            }
            _ => Ok(()),
        }
    }

    async fn on_live(&mut self) -> Result<(), AgentError> {
        let prompt_body = match self.memory.context() {
            Some(context) => format!(
                "Conversation so far:\n{context}\nShare a proactive update even without a new peer message."
            ),
            None => "Start the conversation with a concise, purposeful update.".to_string(),
        };
        self.reply_with_prompt(prompt_body).await
    }

    fn live_interval(&self) -> Duration {
        self.conf.live_interval
    }
}

impl AgentBuilder for LlmAgentConf {
    type A = LlmAgent;

    fn build(&self, client: TimClient) -> Result<Self::A, AgentError> {
        LlmAgent::new(self, client)
    }
}
