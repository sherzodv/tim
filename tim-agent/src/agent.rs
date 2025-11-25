use std::time::Duration;

use async_trait::async_trait;
use tinytemplate::error::Error as TemplateError;
use tokio::time::interval_at;
use tokio::time::Instant;
use tokio::time::MissedTickBehavior;
use tracing::debug;

use crate::tim_client::SpaceEvent;
use crate::tim_client::TimClient;
use crate::tim_client::TimClientConf;
use crate::tim_client::TimClientError;

const MIN_LIVE_INTERVAL: Duration = Duration::from_secs(5);

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

    #[error("memory error: {0}")]
    Memory(String),
}

#[async_trait]
pub trait Agent: Send {
    async fn on_start(&mut self) -> Result<(), AgentError> {
        Ok(())
    }

    async fn on_space_update(&mut self, _: &SpaceEvent) -> Result<(), AgentError> {
        Ok(())
    }

    async fn on_live(&mut self) -> Result<(), AgentError> {
        Ok(())
    }

    fn live_interval(&self) -> Option<Duration> {
        None
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

        let mut live_timer = agent.live_interval().map(|period| {
            let safe_period = period.max(MIN_LIVE_INTERVAL);
            debug!(?period, ?safe_period, "agent live timer configured");
            let mut timer = interval_at(Instant::now() + safe_period, safe_period);
            timer.set_missed_tick_behavior(MissedTickBehavior::Delay);
            timer
        });

        loop {
            if let Some(timer) = live_timer.as_mut() {
                tokio::select! {
                    maybe_update = stream.message() => {
                        let maybe_update = maybe_update?;
                        let Some(update) = maybe_update else {
                            break;
                        };
                        agent.on_space_update(&update).await?;
                    }
                    _ = timer.tick() => {
                        debug!("agent live tick");
                        agent.on_live().await?;
                    }
                }
            } else {
                let maybe_update = stream.message().await?;
                let Some(update) = maybe_update else {
                    break;
                };
                agent.on_space_update(&update).await?;
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
    let mut runner = AgentRunner::new(&client).await;
    let agent = builder.build(client)?;
    runner.start(agent).await
}
