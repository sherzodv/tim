use std::fmt::Debug;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::SecondsFormat;
use serde::Serialize;
use tokio::time::Duration;
use tracing::debug;
use tracing::trace;

use super::ability;
use super::chatgpt::ChatGpt;
use super::llm::Llm;
use super::llm::LlmReq;
use super::llm::LlmRes;
use super::memory::Memory;
use crate::agent::Agent as AgentTrait;
use crate::agent::AgentBuilder;
use crate::agent::AgentError;
use crate::llm::memory::MemoryError;
use crate::llm::prompt::render;
use crate::tim_client::Event;
use crate::tim_client::EventNewMessage;
use crate::tim_client::SpaceEvent;
use crate::tim_client::TimClient;

#[derive(Clone)]
pub struct AgentConf {
    pub sysp: String,
    pub api_key: String,
    pub endpoint: String,
    pub model: String,
    pub temperature: f32,
    pub live_interval: Option<Duration>,
}

pub struct Agent {
    client: TimClient,
    conf: AgentConf,
    llm: Arc<dyn Llm>,
    memory: Memory,
}

impl Debug for AgentConf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentConf")
            .field("userp", &self.sysp)
            .field("endpoint", &self.endpoint)
            .field("model", &self.model)
            .field("temperature", &self.temperature)
            .field("live_interval", &self.live_interval)
            .finish()
    }
}

impl Debug for Agent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Agent")
            .field("client", &self.client)
            .field("conf", &self.conf)
            .finish()
    }
}

#[derive(Debug, Serialize)]
struct AgentPromptContext {
    nick: String,
    history: String,
    now: String,
}

const TIM_HISTORY_TEMPLATE: &str = include_str!("../../prompts/history.md");

impl From<MemoryError> for AgentError {
    fn from(value: MemoryError) -> Self {
        AgentError::Memory(value.to_string())
    }
}

impl Agent {
    pub fn new(conf: &AgentConf, client: TimClient) -> Result<Self, AgentError> {
        let llm: Arc<dyn Llm> = Arc::new(
            ChatGpt::new(
                conf.api_key.clone(),
                conf.endpoint.clone(),
                conf.model.clone(),
                conf.temperature,
            )
            .map_err(|err| AgentError::Llm(err.to_string()))?,
        );
        let memory = Memory::new(client.clone());
        Ok(Self {
            client,
            conf: conf.clone(),
            llm,
            memory,
        })
    }

    async fn ask_llm(&mut self) -> Result<(), AgentError> {
        let history = match self.memory.context().await? {
            Some(context) => context,
            None => "EMPTY_HISTORY".to_string(),
        };
        let nick = self.client.get_me().nick.clone();
        let ctx = AgentPromptContext {
            nick: nick.clone(),
            history: history,
            now: chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
        };
        let sysp = render(&self.conf.sysp, &ctx)?;
        let msg = render(TIM_HISTORY_TEMPLATE, &ctx)?;
        let req = LlmReq {
            sysp: &sysp,
            msg: &msg,
        };
        trace!("{} sending LLM request: {}", nick, req.msg);
        let answer = self
            .llm
            .chat(&req)
            .await
            .map_err(|err| AgentError::Llm(err.to_string()))?;
        match answer {
            super::llm::LlmRes::NoResponse(reason) => {
                debug!("{} chose silence. Reason: {}", nick, reason);
                Ok(())
            }
            LlmRes::Reply(message) => {
                debug!("{} chose to reply: {}", nick, message.chars().take(10).collect::<String>());
                self.client.send_message(&message).await?;
                Ok(())
            }
        }
    }

    async fn render_space_abilities(&mut self) -> Result<Option<String>, AgentError> {
        let abilities = self.client.list_abilities().await?;
        ability::render_space_abilities(&abilities).map_err(AgentError::from)
    }
}

#[async_trait]
impl AgentTrait for Agent {
    async fn on_start(&mut self) -> Result<(), AgentError> {
        let _ = self.render_space_abilities().await?;
        Ok(())
    }

    async fn on_space_update(&mut self, update: &SpaceEvent) -> Result<(), AgentError> {
        match &update.data {
            Some(Event::EventNewMessage(EventNewMessage { message: Some(_) })) => Ok(()),
            _ => Ok(()),
        }
    }

    async fn on_live(&mut self) -> Result<(), AgentError> {
        self.ask_llm().await?;
        Ok(())
    }

    fn live_interval(&self) -> Option<Duration> {
        self.conf.live_interval
    }
}

impl AgentBuilder for AgentConf {
    type A = Agent;

    fn build(&self, client: TimClient) -> Result<Self::A, AgentError> {
        Agent::new(self, client)
    }
}
