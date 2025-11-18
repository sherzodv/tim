use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::RwLock;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use prost_types::Timestamp;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::SendError;

use crate::api::space_event::Data as EventData;
use crate::api::space_event::Metadata as EventMetadata;
use crate::api::CallAbility;
use crate::api::CallAbilityOutcome;
use crate::api::EventCallAbility;
use crate::api::EventCallAbilityOutcome;
use crate::api::EventNewMessage;
use crate::api::Message;
use crate::api::SendMessageReq;
use crate::api::SendMessageRes;
use crate::api::Session;
use crate::api::SpaceEvent;
use crate::api::SubscribeToSpaceReq;

const BUFFER_SIZE: usize = 10;

#[derive(Debug, thiserror::Error)]
pub enum TimSpaceError {
    #[error("Lock poisoned: {0}")]
    LockPoisoned(String),

    #[error("Send failed: {0}")]
    ChannelError(#[from] SendError<SpaceEvent>),
}

#[derive(Debug, Clone)]
struct Subscriber {
    receive_own_messages: bool,
    chan: mpsc::Sender<SpaceEvent>,
    session: Session,
}

pub struct TimSpace {
    msg_counter: AtomicU64,
    upd_counter: AtomicU64,
    subscribers: RwLock<HashMap<String, Subscriber>>,
}

fn update_new_message(
    upd_id: u64,
    msg_id: u64,
    req: &SendMessageReq,
    session: &Session,
) -> SpaceEvent {
    SpaceEvent {
        metadata: event_metadata(upd_id),
        data: Some(EventData::EventNewMessage(EventNewMessage {
            message: Some(Message {
                id: msg_id,
                sender_id: session.timite_id,
                content: req.content.to_string(),
            }),
        })),
    }
}

fn update_call_ability_outcome(upd_id: u64, outcome: &CallAbilityOutcome) -> SpaceEvent {
    SpaceEvent {
        metadata: event_metadata(upd_id),
        data: Some(EventData::EventCallAbilityOutcome(
            EventCallAbilityOutcome {
                call_ability_outcome: Some(outcome.clone()),
            },
        )),
    }
}

fn update_call_ability(upd_id: u64, call_ability: &CallAbility) -> SpaceEvent {
    SpaceEvent {
        metadata: event_metadata(upd_id),
        data: Some(EventData::EventCallAbility(EventCallAbility {
            call_ability: Some(call_ability.clone()),
        })),
    }
}

fn now_timestamp_ms() -> Timestamp {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    Timestamp {
        seconds: now.as_secs() as i64,
        nanos: (now.subsec_millis() * 1_000_000) as i32,
    }
}

fn event_metadata(upd_id: u64) -> Option<EventMetadata> {
    Some(EventMetadata {
        id: upd_id,
        emitted_at: Some(now_timestamp_ms()),
    })
}

impl TimSpace {
    pub fn new() -> TimSpace {
        TimSpace {
            msg_counter: AtomicU64::new(0),
            upd_counter: AtomicU64::new(0),
            subscribers: RwLock::new(HashMap::new()),
        }
    }

    pub async fn process(
        &self,
        req: &SendMessageReq,
        session: &Session,
    ) -> Result<SendMessageRes, TimSpaceError> {
        let snapshot = {
            let guard = self
                .subscribers
                .read()
                .expect("space updates subscribers lock poisoned");
            guard
                .iter()
                .map(|(_, entry)| entry.clone())
                .collect::<Vec<_>>()
        };

        for sub in snapshot {
            if !sub.receive_own_messages && sub.session.timite_id == session.timite_id {
                continue;
            }
            let upd_id = self.upd_counter.fetch_add(1, Ordering::Relaxed);
            let msg_id = self.msg_counter.fetch_add(1, Ordering::Relaxed);
            sub.chan
                .send(update_new_message(upd_id, msg_id, &req, &session))
                .await?;
        }

        Ok(SendMessageRes { error: None })
    }

    pub fn subscribe(
        &self,
        req: &SubscribeToSpaceReq,
        session: &Session,
    ) -> mpsc::Receiver<SpaceEvent> {
        let (sender, receiver) = mpsc::channel(BUFFER_SIZE);
        let mut guard = self
            .subscribers
            .write()
            .expect("space updates subscribers lock poisoned");
        guard.insert(
            session.key.clone(),
            Subscriber {
                receive_own_messages: req.receive_own_messages,
                chan: sender,
                session: session.clone(),
            },
        );
        receiver
    }

    pub async fn publish_call_outcome(
        &self,
        outcome: &CallAbilityOutcome,
        sender_timite_id: u64,
    ) -> Result<(), TimSpaceError> {
        let snapshot = {
            let guard = self
                .subscribers
                .read()
                .expect("space updates subscribers lock poisoned");
            guard
                .iter()
                .map(|(_, entry)| entry.clone())
                .collect::<Vec<_>>()
        };

        for sub in snapshot {
            if !sub.receive_own_messages && sub.session.timite_id == sender_timite_id {
                continue;
            }
            let upd_id = self.upd_counter.fetch_add(1, Ordering::Relaxed);
            sub.chan.send(update_call_ability_outcome(upd_id, outcome)).await?;
        }

        Ok(())
    }

    pub async fn publish_call_ability(
        &self,
        call_ability: &CallAbility,
    ) -> Result<(), TimSpaceError> {
        let snapshot = {
            let guard = self
                .subscribers
                .read()
                .expect("space updates subscribers lock poisoned");
            guard
                .iter()
                .map(|(_, entry)| entry.clone())
                .collect::<Vec<_>>()
        };

        for sub in snapshot {
            let upd_id = self.upd_counter.fetch_add(1, Ordering::Relaxed);
            sub.chan
                .send(update_call_ability(upd_id, call_ability))
                .await?;
        }

        Ok(())
    }
}
