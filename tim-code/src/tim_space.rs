use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::RwLock;

use tokio::sync::mpsc;
use tokio::sync::mpsc::error::SendError;

use crate::api::space_update;
use crate::api::CallAbility;
use crate::api::CallAbilityOutcome;
use crate::api::Message;
use crate::api::SendMessageReq;
use crate::api::SendMessageRes;
use crate::api::Session;
use crate::api::SpaceNewMessage;
use crate::api::SpaceUpdate;
use crate::api::SubscribeToSpaceReq;

const BUFFER_SIZE: usize = 10;

#[derive(Debug, thiserror::Error)]
pub enum TimSpaceError {
    #[error("Lock poisoned: {0}")]
    LockPoisoned(String),

    #[error("Send failed: {0}")]
    ChannelError(#[from] SendError<SpaceUpdate>),
}

#[derive(Debug, Clone)]
struct Subscriber {
    receive_own_messages: bool,
    chan: mpsc::Sender<SpaceUpdate>,
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
) -> SpaceUpdate {
    SpaceUpdate {
        id: upd_id,
        event: Some(space_update::Event::SpaceNewMessage(SpaceNewMessage {
            message: Some(Message {
                id: msg_id,
                sender_id: session.timite_id,
                content: req.content.to_string(),
            }),
        })),
    }
}

fn update_call_outcome(upd_id: u64, outcome: &CallAbilityOutcome) -> SpaceUpdate {
    SpaceUpdate {
        id: upd_id,
        event: Some(space_update::Event::CallAbilityOutcome(outcome.clone())),
    }
}

fn update_call_ability(upd_id: u64, call_ability: &CallAbility) -> SpaceUpdate {
    SpaceUpdate {
        id: upd_id,
        event: Some(space_update::Event::CallAbility(call_ability.clone())),
    }
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
    ) -> mpsc::Receiver<SpaceUpdate> {
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
            sub.chan.send(update_call_outcome(upd_id, outcome)).await?;
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
