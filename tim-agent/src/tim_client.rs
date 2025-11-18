use std::str::FromStr;

pub mod tim_api {
    tonic::include_proto!("tim.api.g1");
}

use tim_api::tim_grpc_api_client::TimGrpcApiClient;
pub use tim_api::{space_event::Event, EventNewMessage, SpaceEvent};
use tim_api::{
    Ability, CallAbilityOutcome, ClientInfo, DeclareAbilitiesReq, ListAbilitiesReq,
    SendCallAbilityOutcomeReq, SendMessageReq, SubscribeToSpaceReq, TimiteAbilities,
    TrustedRegisterReq,
};
use tonic::metadata::errors::InvalidMetadataValue;
use tonic::metadata::{Ascii, MetadataValue};
use tonic::transport::Endpoint;

pub const SESSION_METADATA_KEY: &str = "tim-session-key";

pub struct TimClientConf {
    pub endpoint: String,
    pub nick: String,
    pub provider: String,
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
}

impl TimClient {
    pub async fn new(conf: TimClientConf) -> Result<Self, TimClientError> {
        let endpoint = Endpoint::from_str(&conf.endpoint)?;
        let channel = endpoint.connect().await?;
        let mut client = TimGrpcApiClient::new(channel);

        let register_req = TrustedRegisterReq {
            nick: conf.nick.to_string(),
            client_info: Some(ClientInfo {
                platform: conf.provider.to_string(),
            }),
        };

        let connect_res = client
            .trusted_register(tonic::Request::new(register_req))
            .await?
            .into_inner();

        let session = connect_res.session.ok_or(TimClientError::MissingSession)?;

        let token = MetadataValue::try_from(session.key.clone())?;

        Ok(TimClient {
            client,
            token,
            timite_id: session.timite_id,
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
}
