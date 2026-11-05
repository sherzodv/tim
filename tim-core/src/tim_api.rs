use std::collections::BTreeSet;
use std::sync::Arc;

use tokio::sync::mpsc;
use tracing::debug;
use tracing::field;
use tracing::instrument;
use tracing::Span;

use crate::api::space_event::Data as SpaceEventData;
use crate::api::DeclareAbilitiesReq;
use crate::api::DeclareAbilitiesRes;
use crate::api::ErrorCode;
use crate::api::GetTimelineReq;
use crate::api::GetTimelineRes;
use crate::api::ListAbilitiesRes;
use crate::api::SendCallAbilityOutcomeReq;
use crate::api::SendCallAbilityOutcomeRes;
use crate::api::SendCallAbilityReq;
use crate::api::SendCallAbilityRes;
use crate::api::SendMessageReq;
use crate::api::SendMessageRes;
use crate::api::SendTaskFinishedReq;
use crate::api::SendTaskFinishedRes;
use crate::api::SendTaskStartedReq;
use crate::api::SendTaskStartedRes;
use crate::api::SendTaskStepReq;
use crate::api::SendTaskStepRes;
use crate::api::SendTaskToolCallReq;
use crate::api::SendTaskToolCallRes;
use crate::api::SendTypingStartedReq;
use crate::api::SendTypingStartedRes;
use crate::api::SendTypingStoppedReq;
use crate::api::SendTypingStoppedRes;
use crate::api::Session;
use crate::api::SpaceEvent;
use crate::api::SubscribeToSpaceReq;
use crate::api::TaskFinished;
use crate::api::TaskStep;
use crate::api::TaskToolCall;
use crate::api::Timite;
use crate::api::TrustedConnectReq;
use crate::api::TrustedConnectRes;
use crate::tim_ability::TimAbility;
use crate::tim_ability::TimAbilityError;
use crate::tim_message::TimMessage;
use crate::tim_message::TimMessageError;
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

    #[error("Message error: {0}")]
    MessageError(#[from] TimMessageError),

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
    t_message: Arc<TimMessage>,
}

impl TimApi {
    pub fn new(
        t_session: Arc<TimSession>,
        t_space: Arc<TimSpace>,
        t_timite: Arc<TimTimite>,
        t_ability: Arc<TimAbility>,
        t_message: Arc<TimMessage>,
    ) -> Self {
        Self {
            t_session,
            t_space,
            t_timite,
            t_ability,
            t_message,
        }
    }

    #[instrument(
        skip(self, req),
        level = "debug",
        fields(service = "api", timite_id = field::Empty)
    )]
    pub async fn trusted_connect(
        &self,
        req: &TrustedConnectReq,
    ) -> Result<TrustedConnectRes, TimApiError> {
        let username = req.username.trim();
        if !valid_username(username) {
            return Ok(TrustedConnectRes {
                session: None,
                error: ErrorCode::InvalidUsername.into(),
            });
        }
        let info = req
            .client_info
            .as_ref()
            .ok_or_else(|| TimApiError::InvalidArgError("client info required".into()))?;

        let timite = self.t_timite.connect(username)?;
        Span::current().record("timite_id", timite.id);
        let session = self.t_session.create(&timite, info)?;

        Ok(TrustedConnectRes {
            session: Some(session),
            error: ErrorCode::Unspecified.into(),
        })
    }

    #[instrument(
        skip(self, req, session),
        level = "debug",
        fields(service = "api", timite_id = session.timite_id)
    )]
    pub async fn declare_abilities(
        &self,
        req: &DeclareAbilitiesReq,
        session: &Session,
    ) -> Result<DeclareAbilitiesRes, TimApiError> {
        self.t_timite
            .declare_abilities(session.timite_id, &req.abilities)?;
        Ok(DeclareAbilitiesRes {})
    }

    #[instrument(skip(self), level = "debug", fields(service = "api"))]
    pub async fn list_abilities(&self) -> Result<ListAbilitiesRes, TimApiError> {
        let abilities = self.t_ability.list()?;
        Ok(ListAbilitiesRes { abilities })
    }

    #[instrument(
        skip(self, req, session),
        level = "debug",
        fields(service = "api", timite_id = session.timite_id)
    )]
    pub async fn send_message(
        &self,
        req: &SendMessageReq,
        session: &Session,
    ) -> Result<SendMessageRes, TimApiError> {
        debug!(
            "message received from timite {}: {}",
            session.timite_id, &req.content
        );
        self.t_message.process_message(req, session).await?;
        Ok(SendMessageRes { error: None })
    }

    #[instrument(
        skip(self, req, session),
        level = "debug",
        fields(service = "api", timite_id = session.timite_id)
    )]
    pub async fn subscribe(
        &self,
        req: &SubscribeToSpaceReq,
        session: &Session,
    ) -> Result<mpsc::Receiver<SpaceEvent>, TimApiError> {
        let timite = self.t_timite.get(session.timite_id)?.unwrap_or(Timite {
            id: session.timite_id,
            username: String::new(),
        });
        Ok(self.t_space.subscribe(req, timite)?)
    }

    #[instrument(
        skip(self, req, session),
        level = "debug",
        fields(service = "api", timite_id = session.timite_id)
    )]
    pub fn get_timeline(
        &self,
        req: &GetTimelineReq,
        session: &Session,
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

    #[instrument(
        skip(self, req, session),
        level = "debug",
        fields(service = "api", timite_id = session.timite_id)
    )]
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

    #[instrument(
        skip(self, req, session),
        level = "debug",
        fields(service = "api", timite_id = session.timite_id)
    )]
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
            .publish_call_outcome(outcome, session.timite_id)?;
        Ok(SendCallAbilityOutcomeRes {})
    }

    #[instrument(
        skip(self, _req, session),
        level = "debug",
        fields(service = "api", timite_id = session.timite_id)
    )]
    pub fn send_typing_started(
        &self,
        _req: &SendTypingStartedReq,
        session: &Session,
    ) -> Result<SendTypingStartedRes, TimApiError> {
        self.t_space.publish_typing_started(session.timite_id)?;
        Ok(SendTypingStartedRes {})
    }

    #[instrument(
        skip(self, _req, session),
        level = "debug",
        fields(service = "api", timite_id = session.timite_id)
    )]
    pub fn send_typing_stopped(
        &self,
        _req: &SendTypingStoppedReq,
        session: &Session,
    ) -> Result<SendTypingStoppedRes, TimApiError> {
        self.t_space.publish_typing_stopped(session.timite_id)?;
        Ok(SendTypingStoppedRes {})
    }

    #[instrument(
        skip(self, req, session),
        level = "debug",
        fields(service = "api", timite_id = session.timite_id)
    )]
    pub fn send_task_started(
        &self,
        req: &SendTaskStartedReq,
        session: &Session,
    ) -> Result<SendTaskStartedRes, TimApiError> {
        let task_id = self
            .t_space
            .publish_task_started(session.timite_id, &req.task)?;
        Ok(SendTaskStartedRes { task_id })
    }

    #[instrument(
        skip(self, req, session),
        level = "debug",
        fields(service = "api", timite_id = session.timite_id)
    )]
    pub fn send_task_step(
        &self,
        req: &SendTaskStepReq,
        session: &Session,
    ) -> Result<SendTaskStepRes, TimApiError> {
        self.t_space.publish_task_step(TaskStep {
            timite_id: session.timite_id,
            task_id: req.task_id,
            step: req.step.clone(),
        })?;
        Ok(SendTaskStepRes {})
    }

    #[instrument(
        skip(self, req, session),
        level = "debug",
        fields(service = "api", timite_id = session.timite_id)
    )]
    pub fn send_task_tool_call(
        &self,
        req: &SendTaskToolCallReq,
        session: &Session,
    ) -> Result<SendTaskToolCallRes, TimApiError> {
        let tool_call = req
            .tool_call
            .as_ref()
            .ok_or_else(|| TimApiError::InvalidArgError("tool call required".into()))?;
        self.t_space.publish_task_tool_call(TaskToolCall {
            timite_id: session.timite_id,
            ..tool_call.clone()
        })?;
        Ok(SendTaskToolCallRes {})
    }

    #[instrument(
        skip(self, req, session),
        level = "debug",
        fields(service = "api", timite_id = session.timite_id)
    )]
    pub fn send_task_finished(
        &self,
        req: &SendTaskFinishedReq,
        session: &Session,
    ) -> Result<SendTaskFinishedRes, TimApiError> {
        self.t_space.publish_task_finished(TaskFinished {
            timite_id: session.timite_id,
            task_id: req.task_id,
        })?;
        Ok(SendTaskFinishedRes {})
    }
}

/// Usernames are lowercase handles safe to type after an `@`.
fn valid_username(username: &str) -> bool {
    !username.is_empty()
        && username.len() <= 32
        && username
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
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
            SpaceEventData::EventCallAbilityOutcome(payload) => {
                ids.insert(payload.sender_id);
            }
            SpaceEventData::EventTimiteConnected(payload) => {
                if let Some(timite) = payload.timite.as_ref() {
                    ids.insert(timite.id);
                }
            }
            SpaceEventData::EventTimiteDisconnected(payload) => {
                if let Some(timite) = payload.timite.as_ref() {
                    ids.insert(timite.id);
                }
            }
            SpaceEventData::EventTypingStarted(payload) => {
                ids.extend(payload.typing_started.as_ref().map(|t| t.timite_id));
            }
            SpaceEventData::EventTypingStopped(payload) => {
                ids.extend(payload.typing_stopped.as_ref().map(|t| t.timite_id));
            }
            SpaceEventData::EventTaskStarted(payload) => {
                ids.extend(payload.task_started.as_ref().map(|t| t.timite_id));
            }
            SpaceEventData::EventTaskStep(payload) => {
                ids.extend(payload.task_step.as_ref().map(|t| t.timite_id));
            }
            SpaceEventData::EventTaskToolCall(payload) => {
                ids.extend(payload.task_tool_call.as_ref().map(|t| t.timite_id));
            }
            SpaceEventData::EventTaskFinished(payload) => {
                ids.extend(payload.task_finished.as_ref().map(|t| t.timite_id));
            }
        }
    }
    ids
}
