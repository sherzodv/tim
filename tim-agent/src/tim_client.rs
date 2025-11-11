use std::str::FromStr;

use tonic::metadata::errors::InvalidMetadataValue;
use tonic::metadata::{Ascii, MetadataValue};
use tonic::transport::Endpoint;

pub mod tim_api {
    tonic::include_proto!("tim.api.g1");
}

use tim_api::tim_api_client::TimApiClient;
pub use tim_api::{
    space_update::Event, AuthenticateReq, ClientInfo, SendMessageReq, SpaceNewMessage, SpaceUpdate,
    SubscribeToSpaceReq, Timite,
};

pub const SESSION_METADATA_KEY: &str = "tim-session-key";

pub struct TimClientConf {
    pub endpoint: String,
    pub timite_id: u64,
    pub nick: String,
    pub provider: String,
}

#[derive(Debug, thiserror::Error)]
pub enum TimClientError {
    #[error("tim connect error: {0}")]
    TimConnect(#[from] tonic::transport::Error),

    #[error("tim gprc error: {0}")]
    TimGrpc(#[from] tonic::Status),

    #[error("missing session id in authenticate response")]
    MissingSession,

    #[error("invalid session metadata value: {0}")]
    SessionMetadata(#[from] InvalidMetadataValue),
}

#[derive(Clone)]
pub struct TimClient {
    client: TimApiClient<tonic::transport::Channel>,
    token: MetadataValue<Ascii>,
}

impl TimClient {
    pub async fn new(conf: TimClientConf) -> Result<Self, TimClientError> {
        let endpoint = Endpoint::from_str(&conf.endpoint)?;
        let channel = endpoint.connect().await?;
        let mut client = TimApiClient::new(channel);

        let auth_req = AuthenticateReq {
            timite: Some(Timite {
                id: conf.timite_id,
                nick: conf.nick.to_string(),
            }),
            client_info: Some(ClientInfo {
                platform: conf.provider.to_string(),
            }),
        };

        let auth_res = client
            .authenticate(tonic::Request::new(auth_req))
            .await?
            .into_inner();

        let session_key = auth_res
            .session
            .as_ref()
            .map(|s| s.key.clone())
            .ok_or(TimClientError::MissingSession)?;

        let token = MetadataValue::try_from(session_key)?;

        Ok(TimClient {
            client: client.clone(),
            token,
        })
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

    pub async fn subscribe_to_space(
        &mut self,
    ) -> Result<tonic::Streaming<SpaceUpdate>, TimClientError> {
        let sub_req = SubscribeToSpaceReq {
            receive_own_messages: false,
        };
        let mut sub_req = tonic::Request::new(sub_req);
        sub_req
            .metadata_mut()
            .insert(SESSION_METADATA_KEY, self.token.clone());
        Ok(self.client.subscribe_to_space(sub_req).await?.into_inner())
    }
}
