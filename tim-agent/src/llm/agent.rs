use std::fmt::Debug;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::SecondsFormat;
use serde::Serialize;
use tokio::time::Duration;
use tracing::debug;
use tracing::trace;

use super::ability;
use super::memory::Memory;
use crate::llm::memory::MemoryError;
use crate::llm::prompt::render;
use tim_client::agent::Agent as AgentTrait;
use tim_client::agent::AgentBuilder;
use tim_client::agent::AgentError;
use tim_client::api::space_event::Data as Event;
use tim_client::api::EventNewMessage;
use tim_client::api::SpaceEvent;
use tim_client::client::TimClient;
use tim_llm::llm::build_llm;
use tim_llm::llm::silence_tool;
use tim_llm::llm::Llm;
use tim_llm::llm::LlmConf;
use tim_llm::llm::LlmItem;
use tim_llm::llm::LlmReq;
use tim_llm::llm::LlmRes;

#[derive(Clone)]
pub struct AgentConf {
    pub sysp: String,
    pub provider: String,
    pub llm: LlmConf,
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
            .field("provider", &self.provider)
            .field("llm", &self.llm)
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
    username: String,
    now: String,
}

impl From<MemoryError> for AgentError {
    fn from(value: MemoryError) -> Self {
        AgentError::Agent(value.to_string())
    }
}

impl Agent {
    pub fn new(conf: &AgentConf, client: TimClient) -> Result<Self, AgentError> {
        let llm = build_llm(&conf.provider, &conf.llm)
            .map_err(|err| AgentError::Agent(err.to_string()))?;
        let memory = Memory::new(client.clone());
        Ok(Self {
            client,
            conf: conf.clone(),
            llm,
            memory,
        })
    }

    async fn ask_llm(&mut self) -> Result<(), AgentError> {
        let history: Vec<LlmItem> = self.memory.context().await?;
        let username = self.client.get_me().username;
        let ctx = AgentPromptContext {
            username: username.clone(),
            now: chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        };
        let sysp =
            render(&self.conf.sysp, &ctx).map_err(|err| AgentError::Agent(err.to_string()))?;
        let tools = [silence_tool()];
        let req = LlmReq {
            sysp: &sysp,
            inputs: &history,
            tools: &tools,
        };
        trace!(
            "{} sending LLM request with {} messages",
            username,
            history.len()
        );
        let answer = self
            .llm
            .chat(&req)
            .await
            .map_err(|err| AgentError::Agent(err.to_string()))?;
        match answer {
            LlmRes::NoResponse(reason) => {
                debug!("{} chose silence. Reason: {}", username, reason);
                Ok(())
            }
            LlmRes::Reply(message) => {
                debug!(
                    "{} chose to reply: {}",
                    username,
                    message.chars().take(10).collect::<String>()
                );
                self.client.send_message(&message).await?;
                Ok(())
            }
        }
    }

    async fn render_space_abilities(&mut self) -> Result<Option<String>, AgentError> {
        let abilities = self.client.list_abilities().await?;
        ability::render_space_abilities(&abilities)
            .map_err(|err| AgentError::Agent(err.to_string()))
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
