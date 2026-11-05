use std::time::Duration;

use async_trait::async_trait;
use tokio::time::interval_at;
use tokio::time::sleep;
use tokio::time::Instant;
use tokio::time::Interval;
use tokio::time::MissedTickBehavior;
use tracing::debug;
use tracing::info;
use tracing::warn;

use crate::api::SpaceEvent;
use crate::client::TimClient;
use crate::client::TimClientConf;
use crate::client::TimClientError;

const MIN_LIVE_INTERVAL: Duration = Duration::from_secs(5);
const RECONNECT_DELAY: Duration = Duration::from_secs(2);

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("tim gprc error: {0}")]
    TimGrpc(#[from] tonic::Status),

    #[error("tim client error: {0}")]
    TimClient(#[from] TimClientError),

    /// Raised by the agent itself; ends the agent, unlike transport errors.
    #[error("agent error: {0}")]
    Agent(String),
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

    fn receive_own_messages(&self) -> bool {
        false
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

    /// Runs the agent until it fails; `after_id` resumes the space after an already seen event.
    pub async fn start<A: Agent>(
        &mut self,
        mut agent: A,
        after_id: Option<u64>,
    ) -> Result<(), AgentError> {
        info!("starting agent: {:?}", self.client);
        agent.on_start().await?;
        let mut live_timer = agent.live_interval().map(|period| {
            let safe_period = period.max(MIN_LIVE_INTERVAL);
            info!(?period, ?safe_period, "agent live timer configured");
            let mut timer = interval_at(Instant::now() + safe_period, safe_period);
            timer.set_missed_tick_behavior(MissedTickBehavior::Delay);
            timer
        });
        let mut cursor = after_id;

        loop {
            match self
                .follow_space(&mut agent, live_timer.as_mut(), &mut cursor)
                .await
            {
                Ok(()) => info!("space stream ended, reconnecting"),
                Err(AgentError::TimGrpc(status)) => {
                    warn!(%status, "space stream failed, reconnecting")
                }
                Err(AgentError::TimClient(error)) => {
                    warn!(%error, "space subscription failed, reconnecting")
                }
                Err(error) => return Err(error),
            }
            sleep(RECONNECT_DELAY).await;
        }
    }

    /// Feeds the agent from one subscription until its stream ends or fails.
    async fn follow_space<A: Agent>(
        &mut self,
        agent: &mut A,
        mut live_timer: Option<&mut Interval>,
        cursor: &mut Option<u64>,
    ) -> Result<(), AgentError> {
        let mut stream = self
            .client
            .subscribe_to_space(*cursor, agent.receive_own_messages())
            .await?;
        loop {
            let maybe_update = match live_timer.as_mut() {
                Some(timer) => tokio::select! {
                    maybe_update = stream.message() => maybe_update?,
                    _ = timer.tick() => {
                        debug!("agent live tick");
                        agent.on_live().await?;
                        continue;
                    }
                },
                None => stream.message().await?,
            };
            let Some(update) = maybe_update else {
                return Ok(());
            };
            if let Some(metadata) = &update.metadata {
                *cursor = Some(metadata.id);
            }
            agent.on_space_update(&update).await?;
        }
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
    runner.start(agent, None).await
}
