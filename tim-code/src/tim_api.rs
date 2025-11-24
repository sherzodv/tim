use std::collections::BTreeSet;
use std::sync::Arc;

use tokio::sync::mpsc;
use tracing::debug;

use crate::api::space_event::Data as SpaceEventData;
use crate::api::DeclareAbilitiesReq;
use crate::api::DeclareAbilitiesRes;
use crate::api::GetTimelineReq;
use crate::api::GetTimelineRes;
use crate::api::ListAbilitiesRes;
use crate::api::SendCallAbilityOutcomeReq;
use crate::api::SendCallAbilityOutcomeRes;
use crate::api::SendCallAbilityReq;
use crate::api::SendCallAbilityRes;
use crate::api::SendMessageReq;
use crate::api::SendMessageRes;
use crate::api::Session;
use crate::api::SpaceEvent;
use crate::api::SubscribeToSpaceReq;
use crate::api::Timite;
use crate::api::TrustedConnectReq;
use crate::api::TrustedConnectRes;
use crate::api::TrustedRegisterReq;
use crate::api::TrustedRegisterRes;
use crate::tim_ability::TimAbility;
use crate::tim_ability::TimAbilityError;
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

    #[error("Ability error: {0}")]
    AbilityError(#[from] TimAbilityError),

    #[error(
        "Call ability target mismatch (call ability targeted timite {call_ability_timite} but sender was {sender_timite})"
    )]
    CallAbilityTargetMismatch {
        call_ability_timite: u64,
        sender_timite: u64,
    },

    #[error("Invalid args error: {0}")]
    InvalidArgError(String),
}

#[derive(Clone)]
pub struct TimApi {
    t_session: Arc<TimSession>,
    t_space: Arc<TimSpace>,
    t_timite: Arc<TimTimite>,
    t_ability: Arc<TimAbility>,
}

impl TimApi {
    pub fn new(
        t_session: Arc<TimSession>,
        t_space: Arc<TimSpace>,
        t_timite: Arc<TimTimite>,
        t_ability: Arc<TimAbility>,
    ) -> Self {
        Self {
            t_session,
            t_space,
            t_timite,
            t_ability,
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

    pub async fn declare_abilities(
        &self,
        req: &DeclareAbilitiesReq,
        session: &Session,
    ) -> Result<DeclareAbilitiesRes, TimApiError> {
        self.t_timite
            .declare_abilities(session.timite_id, &req.abilities)?;
        Ok(DeclareAbilitiesRes {})
    }

    pub async fn list_abilities(&self) -> Result<ListAbilitiesRes, TimApiError> {
        let abilities = self.t_ability.list()?;
        Ok(ListAbilitiesRes { abilities })
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
    ) -> mpsc::Receiver<SpaceEvent> {
        self.t_space.subscribe(req, &session)
    }

    pub fn get_timeline(
        &self,
        req: &GetTimelineReq,
        _session: &Session,
    ) -> Result<GetTimelineRes, TimApiError> {
        let events = self.t_space.timeline(req.offset, req.size)?;
        let mut timites: Vec<Timite> = Vec::new();
        for timite_id in collect_timite_ids(&events) {
            if let Some(timite) = self.t_timite.get(timite_id)? {
                timites.push(timite);
            }
        }
        Ok(GetTimelineRes {
            offset: req.offset,
            size: req.size,
            events,
            timites,
        })
    }

    pub async fn send_call_ability(
        &self,
        req: &SendCallAbilityReq,
        session: &Session,
    ) -> Result<SendCallAbilityRes, TimApiError> {
        let call_ability = req
            .call_ability
            .as_ref()
            .ok_or_else(|| TimApiError::InvalidArgError("call ability required".into()))?;
        let call_ability_id = self
            .t_ability
            .process_call_ability(call_ability, session)
            .await?;
        Ok(SendCallAbilityRes { call_ability_id })
    }

    pub async fn send_call_ability_outcome(
        &self,
        req: &SendCallAbilityOutcomeReq,
        session: &Session,
    ) -> Result<SendCallAbilityOutcomeRes, TimApiError> {
        let outcome = req
            .outcome
            .as_ref()
            .ok_or_else(|| TimApiError::InvalidArgError("outcome payload required".into()))?;
        let call_ability = self.t_ability.find_call_ability(outcome.call_ability_id)?;
        if call_ability.timite_id != session.timite_id {
            return Err(TimApiError::CallAbilityTargetMismatch {
                call_ability_timite: call_ability.timite_id,
                sender_timite: session.timite_id,
            });
        }
        self.t_space
            .publish_call_outcome(outcome, session.timite_id)
            .await?;
        Ok(SendCallAbilityOutcomeRes {})
    }
}

fn collect_timite_ids(events: &[SpaceEvent]) -> BTreeSet<u64> {
    let mut ids = BTreeSet::new();
    for event in events {
        let Some(data) = event.data.as_ref() else {
            continue;
        };
        match data {
            SpaceEventData::EventNewMessage(payload) => {
                if let Some(message) = payload.message.as_ref() {
                    ids.insert(message.sender_id);
                }
            }
            SpaceEventData::EventCallAbility(payload) => {
                if let Some(call_ability) = payload.call_ability.as_ref() {
                    ids.insert(call_ability.sender_id);
                    ids.insert(call_ability.timite_id);
                }
            }
            SpaceEventData::EventCallAbilityOutcome(_) => {}
        }
    }
    ids
}
