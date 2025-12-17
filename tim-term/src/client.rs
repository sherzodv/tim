use std::str::FromStr;

pub mod tim_api {
    tonic::include_proto!("tim.api.g1");
}

pub use tim_api::space_event::Data as EventData;
use tim_api::tim_grpc_api_client::TimGrpcApiClient;
pub use tim_api::CallAbility;
pub use tim_api::CallAbilityOutcome;
use tim_api::ClientInfo;
use tim_api::GetTimelineReq;
pub use tim_api::GetTimelineRes;
use tim_api::ListAbilitiesReq;
pub use tim_api::Message;
use tim_api::SendMessageReq;
pub use tim_api::SpaceEvent;
use tim_api::SubscribeToSpaceReq;
pub use tim_api::Timite;
pub use tim_api::TimiteAbilities;
use tim_api::TrustedConnectReq;
use tim_api::TrustedRegisterReq;
use tonic::metadata::Ascii;
use tonic::metadata::MetadataValue;
use tonic::transport::Endpoint;

use crate::error::{Error, Result};

pub const SESSION_METADATA_KEY: &str = "tim-session-key";

#[derive(Clone)]
pub struct ClientConfig {
    pub endpoint: String,
    pub nick: String,
    pub timite_id: Option<u64>,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            endpoint: "http://127.0.0.1:8787".to_string(),
            nick: "terminal-user".to_string(),
            timite_id: None,
        }
    }
}

#[derive(Clone)]
pub struct TimClient {
    client: TimGrpcApiClient<tonic::transport::Channel>,
    token: MetadataValue<Ascii>,
    timite_id: u64,
}

impl TimClient {
    pub async fn connect(conf: ClientConfig) -> Result<Self> {
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
                        platform: "tim-term".to_string(),
                    }),
                };
                let res = client
                    .trusted_connect(tonic::Request::new(connect_req))
                    .await?
                    .into_inner();

                if let Some(session) = res.session {
                    session
                } else {
                    let register_req = TrustedRegisterReq {
                        nick: conf.nick.clone(),
                        client_info: Some(ClientInfo {
                            platform: "tim-term".to_string(),
                        }),
                    };
                    client
                        .trusted_register(tonic::Request::new(register_req))
                        .await?
                        .into_inner()
                        .session
                        .ok_or(Error::MissingSession)?
                }
            }
            None => {
                let register_req = TrustedRegisterReq {
                    nick: conf.nick.clone(),
                    client_info: Some(ClientInfo {
                        platform: "tim-term".to_string(),
                    }),
                };
                client
                    .trusted_register(tonic::Request::new(register_req))
                    .await?
                    .into_inner()
                    .session
                    .ok_or(Error::MissingSession)?
            }
        };

        let token = MetadataValue::try_from(session.key.clone())?;

        Ok(TimClient {
            client,
            token,
            timite_id: session.timite_id,
        })
    }

    pub fn timite_id(&self) -> u64 {
        self.timite_id
    }

    pub async fn send_message(&mut self, content: &str) -> Result<()> {
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

    pub async fn subscribe_to_space(&mut self) -> Result<tonic::Streaming<SpaceEvent>> {
        let sub_req = SubscribeToSpaceReq {
            receive_own_messages: true,
        };
        let mut req = tonic::Request::new(sub_req);
        req.metadata_mut()
            .insert(SESSION_METADATA_KEY, self.token.clone());
        Ok(self.client.subscribe_to_space(req).await?.into_inner())
    }

    pub async fn get_timeline(&mut self, offset: u64, size: u32) -> Result<GetTimelineRes> {
        let mut req = tonic::Request::new(GetTimelineReq { offset, size });
        req.metadata_mut()
            .insert(SESSION_METADATA_KEY, self.token.clone());
        Ok(self.client.get_timeline(req).await?.into_inner())
    }

    pub async fn list_abilities(&mut self) -> Result<Vec<TimiteAbilities>> {
        let mut req = tonic::Request::new(ListAbilitiesReq { timite_id: None });
        req.metadata_mut()
            .insert(SESSION_METADATA_KEY, self.token.clone());
        let res = self.client.list_abilities(req).await?.into_inner();
        Ok(res.abilities)
    }
}
