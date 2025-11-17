use async_trait::async_trait;

use crate::tim_client::tim_api::CallAbility;
use crate::tim_client::Event;
use crate::tim_client::SpaceNewMessage;

use crate::tim_client::{TimClient, TimClientConf, TimClientError};
use tinytemplate::error::Error as TemplateError;

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("llm error: {0}")]
    Llm(String),

    #[error("tim gprc error: {0}")]
    TimGrpc(#[from] tonic::Status),

    #[error("tim client error: {0}")]
    TimeClientError(#[from] TimClientError),

    #[error("crawler error: {0}")]
    Crawler(String),

    #[error("template error: {0}")]
    Template(#[from] TemplateError),
}

#[async_trait]
pub trait Agent: Send {
    async fn on_start(&mut self) -> Result<(), AgentError> {
        Ok(())
    }

    async fn on_space_message(&mut self, sender_id: u64, content: &str) -> Result<(), AgentError>;

    async fn on_call_ability(&mut self, _call: &CallAbility) -> Result<(), AgentError> {
        Ok(())
    }
}

pub struct AgentRunner {
    client: TimClient,
}

impl AgentRunner {
    pub async fn new(client: &TimClient) -> AgentRunner {
        AgentRunner {
            client: client.clone(),
        }
    }

    pub async fn start<A: Agent>(&mut self, mut agent: A) -> Result<(), AgentError> {
        let mut stream = self.client.subscribe_to_space().await?;

        agent.on_start().await?;

        while let Some(upd) = stream.message().await? {
            match upd.event {
                Some(Event::SpaceNewMessage(SpaceNewMessage {
                    message: Some(message),
                })) => {
                    agent
                        .on_space_message(message.sender_id, &message.content)
                        .await?;
                }
                Some(Event::CallAbility(call)) => {
                    if call.timite_id == self.client.timite_id() {
                        agent.on_call_ability(&call).await?;
                    }
                }
                _ => {
                    eprintln!("Unhandled space update: {:?}", upd);
                }
            }
        }

        Ok(())
    }
}

pub trait AgentBuilder {
    type A: Agent;
    fn build(&self, tim_client: TimClient) -> Result<Self::A, AgentError>;
}

pub async fn spawn<B: AgentBuilder>(conf: TimClientConf, builder: B) -> Result<(), AgentError> {
    let client = TimClient::new(conf).await?;
    let mut connector = AgentRunner::new(&client).await;
    let agent = builder.build(client)?;
    connector.start(agent).await
}
