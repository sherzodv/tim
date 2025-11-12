use std::sync::Arc;

use tokio::sync::mpsc;
use tracing::debug;

use crate::api::DeclareCapabilitiesReq;
use crate::api::DeclareCapabilitiesRes;
use crate::api::ListCapabilitiesRes;
use crate::api::SendMessageReq;
use crate::api::SendMessageRes;
use crate::api::Session;
use crate::api::SpaceUpdate;
use crate::api::SubscribeToSpaceReq;
use crate::api::TrustedConnectReq;
use crate::api::TrustedConnectRes;
use crate::api::TrustedRegisterReq;
use crate::api::TrustedRegisterRes;
use crate::tim_capability::TimCapability;
use crate::tim_capability::TimCapabilityError;
use crate::tim_session::TimSession;
use crate::tim_session::TimSessionError;
use crate::tim_space::TimSpace;
use crate::tim_space::TimSpaceError;
use crate::tim_timite::TimTimite;
use crate::tim_timite::TimTimiteError;

#[derive(Debug, thiserror::Error)]
pub enum TimApiError {
    #[error("Session error: {0}")]
    SessionError(#[from] TimSessionError),

    #[error("Timite error: {0}")]
    TimiteError(#[from] TimTimiteError),

    #[error("Space error: {0}")]
    SpaceError(#[from] TimSpaceError),

    #[error("Capability error: {0}")]
    CapabilityError(#[from] TimCapabilityError),

    #[error("Invalid args error: {0}")]
    InvalidArgError(String),
}

#[derive(Clone)]
pub struct TimApi {
    t_session: Arc<TimSession>,
    t_space: Arc<TimSpace>,
    t_timite: Arc<TimTimite>,
    t_capability: Arc<TimCapability>,
}

impl TimApi {
    pub fn new(
        t_session: Arc<TimSession>,
        t_space: Arc<TimSpace>,
        t_timite: Arc<TimTimite>,
        t_capability: Arc<TimCapability>,
    ) -> Self {
        Self {
            t_session,
            t_space,
            t_timite,
            t_capability,
        }
    }

    pub async fn trusted_register(
        &self,
        req: &TrustedRegisterReq,
    ) -> Result<TrustedRegisterRes, TimApiError> {
        let timite = self.t_timite.create(&req.nick)?;

        let info = req
            .client_info
            .as_ref()
            .ok_or_else(|| TimApiError::InvalidArgError("client info required".into()))?;

        let session = self.t_session.create(&timite, info)?;

        Ok(TrustedRegisterRes {
            session: Some(session),
        })
    }

    pub async fn trusted_connect(
        &self,
        req: &TrustedConnectReq,
    ) -> Result<TrustedConnectRes, TimApiError> {
        let timite = req
            .timite
            .as_ref()
            .ok_or_else(|| TimApiError::InvalidArgError("timite required".into()))?;

        let info = req
            .client_info
            .as_ref()
            .ok_or_else(|| TimApiError::InvalidArgError("client info required".into()))?;

        let session = self.t_session.create(&timite, info)?;

        Ok(TrustedConnectRes {
            session: Some(session),
        })
    }

    pub async fn declare_capabilities(
        &self,
        req: &DeclareCapabilitiesReq,
        session: &Session,
    ) -> Result<DeclareCapabilitiesRes, TimApiError> {
        self.t_timite
            .declare_capabilities(session.timite_id, &req.capabilities)?;
        Ok(DeclareCapabilitiesRes {})
    }

    pub async fn list_capabilities(&self) -> Result<ListCapabilitiesRes, TimApiError> {
        let capabilities = self.t_capability.list()?;
        Ok(ListCapabilitiesRes { capabilities })
    }

    pub async fn send_message(
        &self,
        req: &SendMessageReq,
        session: &Session,
    ) -> Result<SendMessageRes, TimApiError> {
        debug!(
            "message received from timite {}: {}",
            session.timite_id, &req.content
        );
        Ok(self.t_space.process(req, session).await?)
    }

    pub fn subscribe(
        &self,
        req: &SubscribeToSpaceReq,
        session: &Session,
    ) -> mpsc::Receiver<SpaceUpdate> {
        self.t_space.subscribe(req, &session)
    }
}
