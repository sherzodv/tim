use std::fmt::Debug;
use std::str::FromStr;

pub mod tim_api {
    tonic::include_proto!("tim.api.g1");
}

use futures::stream;
pub use tim_api::space_event::Data as Event;
use tim_api::tim_grpc_api_client::TimGrpcApiClient;
use tim_api::Ability;
use tim_api::CallAbilityOutcome;
use tim_api::ClientInfo;
use tim_api::DeclareAbilitiesReq;
pub use tim_api::EventNewMessage;
use tim_api::GetTimelineReq;
use tim_api::GetTimelineRes;
use tim_api::ListAbilitiesReq;
use tim_api::SendCallAbilityOutcomeReq;
use tim_api::SendMessageReq;
pub use tim_api::SpaceEvent;
use tim_api::SubscribeToSpaceReq;
use tim_api::TimiteAbilities;
use tim_api::TrustedConnectReq;
use tim_api::TrustedRegisterReq;
use tokio_stream::Stream;
use tonic::metadata::errors::InvalidMetadataValue;
use tonic::metadata::Ascii;
use tonic::metadata::MetadataValue;
use tonic::transport::Endpoint;

use crate::tim_client::tim_api::ErrorCode;
use crate::tim_client::tim_api::Timite;

pub const SESSION_METADATA_KEY: &str = "tim-session-key";

#[derive(Clone)]
pub struct TimClientConf {
    pub endpoint: String,
    pub nick: String,
    pub provider: String,
    pub timite_id: Option<u64>,
}

#[derive(Debug, thiserror::Error)]
pub enum TimClientError {
    #[error("tim connect error: {0}")]
    TimConnect(#[from] tonic::transport::Error),

    #[error("tim gprc error: {0}")]
    TimGrpc(#[from] tonic::Status),

    #[error("missing session key in trusted register response")]
    MissingSession,

    #[error("invalid session metadata value: {0}")]
    SessionMetadata(#[from] InvalidMetadataValue),
}

#[derive(Clone)]
pub struct TimClient {
    client: TimGrpcApiClient<tonic::transport::Channel>,
    token: MetadataValue<Ascii>,
    timite_id: u64,
    nick: String,
}

impl Debug for TimClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("tim")
            .field("nick", &self.nick)
            .field("timite_id", &self.timite_id)
            .finish()
    }
}

impl TimClient {
    pub async fn new(conf: TimClientConf) -> Result<Self, TimClientError> {
        let endpoint = Endpoint::from_str(&conf.endpoint)?;
        let channel = endpoint.connect().await?;
        let mut client = TimGrpcApiClient::new(channel);

        let session = match conf.timite_id {
            Some(timite_id) => {
                let connect_req = TrustedConnectReq {
                    timite: Some(Timite {
                        id: timite_id,
                        nick: conf.nick.clone(),
                    }),
                    client_info: Some(ClientInfo {
                        platform: conf.provider.to_string(),
                    }),
                };
                let connect_res = client
                    .trusted_connect(tonic::Request::new(connect_req))
                    .await?
                    .into_inner();

                if let Some(session) = connect_res.session {
                    session
                } else {
                    let err_code =
                        ErrorCode::try_from(connect_res.error).unwrap_or(ErrorCode::Unspecified);
                    if err_code == ErrorCode::TimiteNotFound {
                        let register_req = TrustedRegisterReq {
                            nick: conf.nick.to_string(),
                            client_info: Some(ClientInfo {
                                platform: conf.provider.to_string(),
                            }),
                        };
                        client
                            .trusted_register(tonic::Request::new(register_req))
                            .await?
                            .into_inner()
                            .session
                            .ok_or(TimClientError::MissingSession)?
                    } else {
                        return Err(TimClientError::MissingSession);
                    }
                }
            }
            None => {
                let register_req = TrustedRegisterReq {
                    nick: conf.nick.to_string(),
                    client_info: Some(ClientInfo {
                        platform: conf.provider.to_string(),
                    }),
                };
                client
                    .trusted_register(tonic::Request::new(register_req))
                    .await?
                    .into_inner()
                    .session
                    .ok_or(TimClientError::MissingSession)?
            }
        };

        let token = MetadataValue::try_from(session.key.clone())?;

        Ok(TimClient {
            client,
            token,
            timite_id: session.timite_id,
            nick: conf.nick,
        })
    }

    pub fn get_me(&self) -> Timite {
        Timite {
            id: self.timite_id,
            nick: self.nick.clone(),
        }
    }

    pub async fn send_message(&mut self, content: &str) -> Result<(), TimClientError> {
        let trimmed = content.trim();
        if trimmed.is_empty() {
            return Ok(());
        }
        let mut req = tonic::Request::new(SendMessageReq {
            content: trimmed.to_string(),
        });
        req.metadata_mut()
            .insert(SESSION_METADATA_KEY, self.token.clone());
        self.client.send_message(req).await?;
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
    ) -> Result<tonic::Streaming<SpaceEvent>, TimClientError> {
        let sub_req = SubscribeToSpaceReq {
            receive_own_messages: false,
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
        let mut req = tonic::Request::new(GetTimelineReq { offset, size });
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
                offset: 0,
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
                        state.offset = state.offset.saturating_add(len);
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
