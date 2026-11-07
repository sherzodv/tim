use std::fmt::Debug;
use std::str::FromStr;

use crate::api::tim_grpc_api_client::TimGrpcApiClient;
use crate::api::Ability;
use crate::api::CallAbilityOutcome;
use crate::api::ClientInfo;
use crate::api::DeclareAbilitiesReq;
use crate::api::GetTimelineReq;
use crate::api::GetTimelineRes;
use crate::api::ListAbilitiesReq;
use crate::api::SendCallAbilityOutcomeReq;
use crate::api::SendMessageReq;
use crate::api::SendTaskFinishedReq;
use crate::api::SendTaskModelEventReq;
use crate::api::SendTaskStartedReq;
use crate::api::SendTaskStepReq;
use crate::api::SendTaskToolCallReq;
use crate::api::SendTypingStartedReq;
use crate::api::SendTypingStoppedReq;
use crate::api::SpaceEvent;
use crate::api::SubscribeToSpaceReq;
use crate::api::TaskModelEvent;
use crate::api::TaskToolCall;
use crate::api::TimiteAbilities;
use crate::api::TrustedConnectReq;
use futures::stream;
use tokio_stream::Stream;
use tonic::metadata::errors::InvalidMetadataValue;
use tonic::metadata::Ascii;
use tonic::metadata::MetadataValue;
use tonic::transport::Endpoint;

use crate::api::ErrorCode;
use crate::api::Timite;

pub const SESSION_METADATA_KEY: &str = "tim-session-key";

#[derive(Clone)]
pub struct TimClientConf {
    pub endpoint: String,
    /// Unique handle the timite connects as and is addressed by.
    pub username: String,
    pub provider: String,
}

#[derive(Debug, thiserror::Error)]
pub enum TimClientError {
    #[error("tim connect error: {0}")]
    TimConnect(#[from] tonic::transport::Error),

    #[error("tim gprc error: {0}")]
    TimGrpc(#[from] tonic::Status),

    #[error("missing session in trusted connect response")]
    MissingSession,

    #[error("tim rejected the connection: {0:?}")]
    Rejected(ErrorCode),

    #[error("invalid session metadata value: {0}")]
    SessionMetadata(#[from] InvalidMetadataValue),
}

#[derive(Clone)]
pub struct TimClient {
    client: TimGrpcApiClient<tonic::transport::Channel>,
    token: MetadataValue<Ascii>,
    timite_id: u64,
    username: String,
    branch_id: Option<u64>,
}

impl Debug for TimClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("tim")
            .field("username", &self.username)
            .field("timite_id", &self.timite_id)
            .finish()
    }
}

impl TimClient {
    pub async fn new(conf: TimClientConf) -> Result<Self, TimClientError> {
        let endpoint = Endpoint::from_str(&conf.endpoint)?;
        let channel = endpoint.connect().await?;
        let mut client = TimGrpcApiClient::new(channel);

        let connect_req = TrustedConnectReq {
            username: conf.username.clone(),
            client_info: Some(ClientInfo {
                platform: conf.provider.to_string(),
            }),
        };
        let connect_res = client
            .trusted_connect(tonic::Request::new(connect_req))
            .await?
            .into_inner();
        let error = ErrorCode::try_from(connect_res.error).unwrap_or(ErrorCode::Unspecified);
        if error != ErrorCode::Unspecified {
            return Err(TimClientError::Rejected(error));
        }
        let session = connect_res.session.ok_or(TimClientError::MissingSession)?;

        let token = MetadataValue::try_from(session.key.clone())?;

        Ok(TimClient {
            client,
            token,
            timite_id: session.timite_id,
            username: conf.username,
            branch_id: None,
        })
    }

    /// A client scoped to one branch; clones keep independent branch selections.
    pub fn in_branch(&self, branch_id: u64) -> Self {
        Self {
            branch_id: Some(branch_id),
            ..self.clone()
        }
    }

    pub fn branch_id(&self) -> u64 {
        self.branch_id.unwrap_or(0)
    }

    pub fn get_me(&self) -> Timite {
        Timite {
            id: self.timite_id,
            username: self.username.clone(),
        }
    }

    pub async fn send_message(&mut self, content: &str) -> Result<(), TimClientError> {
        let trimmed = content.trim();
        if trimmed.is_empty() {
            return Ok(());
        }
        let mut req = tonic::Request::new(SendMessageReq {
            content: trimmed.to_string(),
            branch_id: self.branch_id(),
        });
        req.metadata_mut()
            .insert(SESSION_METADATA_KEY, self.token.clone());
        self.client.send_message(req).await?;
        Ok(())
    }

    pub async fn send_typing_started(&mut self) -> Result<(), TimClientError> {
        let mut req = tonic::Request::new(SendTypingStartedReq {
            branch_id: self.branch_id(),
        });
        req.metadata_mut()
            .insert(SESSION_METADATA_KEY, self.token.clone());
        self.client.send_typing_started(req).await?;
        Ok(())
    }

    pub async fn send_typing_stopped(&mut self) -> Result<(), TimClientError> {
        let mut req = tonic::Request::new(SendTypingStoppedReq {
            branch_id: self.branch_id(),
        });
        req.metadata_mut()
            .insert(SESSION_METADATA_KEY, self.token.clone());
        self.client.send_typing_stopped(req).await?;
        Ok(())
    }

    /// Announces a task and returns its id for the steps and the finish.
    pub async fn send_task_started(
        &mut self,
        task: &str,
        source_event_id: u64,
    ) -> Result<u64, TimClientError> {
        let mut req = tonic::Request::new(SendTaskStartedReq {
            task: task.to_string(),
            branch_id: self.branch_id(),
            source_event_id,
        });
        req.metadata_mut()
            .insert(SESSION_METADATA_KEY, self.token.clone());
        let res = self.client.send_task_started(req).await?;
        Ok(res.into_inner().task_id)
    }

    pub async fn send_task_step(&mut self, task_id: u64, step: &str) -> Result<(), TimClientError> {
        let mut req = tonic::Request::new(SendTaskStepReq {
            task_id,
            step: step.to_string(),
        });
        req.metadata_mut()
            .insert(SESSION_METADATA_KEY, self.token.clone());
        self.client.send_task_step(req).await?;
        Ok(())
    }

    /// Records a finished tool call of the task; `timite_id` is set by the server.
    pub async fn send_task_tool_call(
        &mut self,
        tool_call: TaskToolCall,
    ) -> Result<(), TimClientError> {
        let mut req = tonic::Request::new(SendTaskToolCallReq {
            tool_call: Some(tool_call),
        });
        req.metadata_mut()
            .insert(SESSION_METADATA_KEY, self.token.clone());
        self.client.send_task_tool_call(req).await?;
        Ok(())
    }

    /// Records a raw coding engine event; `timite_id` is set by the server.
    pub async fn send_task_model_event(
        &mut self,
        model_event: TaskModelEvent,
    ) -> Result<(), TimClientError> {
        let mut req = tonic::Request::new(SendTaskModelEventReq {
            model_event: Some(model_event),
        });
        req.metadata_mut()
            .insert(SESSION_METADATA_KEY, self.token.clone());
        self.client.send_task_model_event(req).await?;
        Ok(())
    }

    pub async fn send_task_finished(
        &mut self,
        task_id: u64,
        error: Option<String>,
        duration_ms: Option<u64>,
    ) -> Result<(), TimClientError> {
        let mut req = tonic::Request::new(SendTaskFinishedReq {
            task_id,
            error,
            duration_ms,
        });
        req.metadata_mut()
            .insert(SESSION_METADATA_KEY, self.token.clone());
        self.client.send_task_finished(req).await?;
        Ok(())
    }

    pub async fn declare_abilities(
        &mut self,
        abilities: Vec<Ability>,
    ) -> Result<(), TimClientError> {
        let mut req = tonic::Request::new(DeclareAbilitiesReq { abilities });
        req.metadata_mut()
            .insert(SESSION_METADATA_KEY, self.token.clone());
        self.client.declare_abilities(req).await?;
        Ok(())
    }

    pub async fn send_call_ability_outcome(
        &mut self,
        outcome: &CallAbilityOutcome,
    ) -> Result<(), TimClientError> {
        let mut req = tonic::Request::new(SendCallAbilityOutcomeReq {
            outcome: Some(outcome.clone()),
        });
        req.metadata_mut()
            .insert(SESSION_METADATA_KEY, self.token.clone());
        self.client.send_call_ability_outcome(req).await?;
        Ok(())
    }

    pub async fn list_abilities(&mut self) -> Result<Vec<TimiteAbilities>, TimClientError> {
        let mut req = tonic::Request::new(ListAbilitiesReq { timite_id: None });
        req.metadata_mut()
            .insert(SESSION_METADATA_KEY, self.token.clone());
        let res = self.client.list_abilities(req).await?.into_inner();
        Ok(res.abilities)
    }

    pub fn timite_id(&self) -> u64 {
        self.timite_id
    }

    pub async fn subscribe_to_space(
        &mut self,
        after_id: Option<u64>,
        receive_own_messages: bool,
    ) -> Result<tonic::Streaming<SpaceEvent>, TimClientError> {
        let sub_req = SubscribeToSpaceReq {
            receive_own_messages,
            after_id,
            branch_id: self.branch_id,
        };
        let mut sub_req = tonic::Request::new(sub_req);
        sub_req
            .metadata_mut()
            .insert(SESSION_METADATA_KEY, self.token.clone());
        Ok(self.client.subscribe_to_space(sub_req).await?.into_inner())
    }

    pub async fn get_timeline(
        &mut self,
        offset: u64,
        size: u32,
    ) -> Result<GetTimelineRes, TimClientError> {
        let mut req = tonic::Request::new(GetTimelineReq {
            offset,
            size,
            branch_id: self.branch_id,
        });
        req.metadata_mut()
            .insert(SESSION_METADATA_KEY, self.token.clone());
        let res = self.client.get_timeline(req).await?.into_inner();
        Ok(res)
    }

    pub fn timeline_stream(
        &mut self,
        page_size: u32,
    ) -> impl Stream<Item = Result<GetTimelineRes, TimClientError>> + '_ {
        struct State<'a> {
            client: &'a mut TimClient,
            offset: u64,
            page_size: u32,
            finished: bool,
        }
        stream::unfold(
            State {
                client: self,
                offset: 1,
                page_size,
                finished: false,
            },
            |mut state| async move {
                if state.finished || state.page_size == 0 {
                    return None;
                }
                let res = state
                    .client
                    .get_timeline(state.offset, state.page_size)
                    .await;
                match res {
                    Ok(res) => {
                        if res.events.is_empty() {
                            state.finished = true;
                            return None;
                        }
                        let len = res.events.len() as u64;
                        state.offset = res
                            .events
                            .last()
                            .and_then(|event| event.metadata.as_ref())
                            .map_or(state.offset, |meta| meta.id + 1);
                        if len < state.page_size as u64 {
                            state.finished = true;
                        }
                        Some((Ok(res), state))
                    }
                    Err(err) => {
                        state.finished = true;
                        Some((Err(err), state))
                    }
                }
            },
        )
    }
}
