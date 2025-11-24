use std::sync::Arc;

use async_trait::async_trait;
use tokio::time::Duration;
use tracing::warn;

use super::ability;
use super::chatgpt::ChatGpt;
use super::llm::Llm;
use super::llm::LlmReq;
use super::memory::Memory;
use crate::agent::Agent as AgentTrait;
use crate::agent::AgentBuilder;
use crate::agent::AgentError;
use crate::tim_client::Event;
use crate::tim_client::EventNewMessage;
use crate::tim_client::SpaceEvent;
use crate::tim_client::TimClient;

#[derive(Clone)]
pub struct AgentConf {
    pub userp: String,
    pub api_key: String,
    pub endpoint: String,
    pub model: String,
    pub temperature: f32,
    pub live_interval: Duration,
}

pub struct Agent {
    client: TimClient,
    conf: AgentConf,
    llm: Arc<dyn Llm>,
    memory: Memory,
}

const TIM_SYSTEM_PROMPT: &str = include_str!("../../prompts/tim-sys.md");

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

    async fn reply_with_prompt(&mut self, prompt_body: String) -> Result<(), AgentError> {
        let req = LlmReq {
            sysp: TIM_SYSTEM_PROMPT,
            userp: &self.conf.userp,
            msg: &prompt_body,
        };
        let answer = self
            .llm
            .chat(&req)
            .await
            .map_err(|err| AgentError::Llm(err.to_string()))?;
        self.client.send_message(&answer.message).await?;
        Ok(())
    }

    async fn handle_peer_message(&mut self, content: String) -> Result<(), AgentError> {
        let prompt_body = match self.memory.context().await {
            Ok(Some(context)) => {
                format!("Conversation so far:\n{context}\nRespond to the latest peer message.")
            }
            Ok(None) => content.trim().to_string(),
            Err(err) => {
                warn!("failed to load conversation context: {err}");
                content.trim().to_string()
            }
        };
        self.reply_with_prompt(prompt_body).await
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
            Some(Event::EventNewMessage(EventNewMessage {
                message: Some(message),
            })) => {
                let content = message.content.clone();
                self.handle_peer_message(content).await
            }
            _ => Ok(()),
        }
    }

    async fn on_live(&mut self) -> Result<(), AgentError> {
        let prompt_body = match self.memory.context().await {
            Ok(Some(context)) => format!(
                "Conversation so far:\n{context}\nShare a proactive update even without a new peer message."
            ),
            Ok(None) => {
                "Start the conversation with a concise, purposeful update.".to_string()
            }
            Err(err) => {
                warn!("failed to load conversation context: {err}");
                "Start the conversation with a concise, purposeful update.".to_string()
            }
        };
        self.reply_with_prompt(prompt_body).await
    }

    fn live_interval(&self) -> Duration {
        self.conf.live_interval
    }
}

impl AgentBuilder for AgentConf {
    type A = Agent;

    fn build(&self, client: TimClient) -> Result<Self::A, AgentError> {
        Agent::new(self, client)
    }
}
