use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::Arc;
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
use crate::api::EventTimiteConnected;
use crate::api::EventTimiteDisconnected;
use crate::api::Message;
use crate::api::Session;
use crate::api::SpaceEvent;
use crate::api::SubscribeToSpaceReq;
use crate::api::Timite;
use crate::tim_storage::TimStorage;
use crate::tim_storage::TimStorageError;

const BUFFER_SIZE: usize = 10;

#[derive(Debug, thiserror::Error)]
pub enum TimSpaceError {
    #[error("Lock poisoned: {0}")]
    LockPoisoned(String),

    #[error("Send failed: {0}")]
    ChannelError(#[from] SendError<SpaceEvent>),

    #[error("Timeline error: {0}")]
    Timeline(#[from] TimStorageError),
}

#[derive(Debug, Clone)]
struct Subscriber {
    receive_own_messages: bool,
    chan: mpsc::Sender<SpaceEvent>,
    session: Session,
    timite: Timite,
}

pub struct TimSpace {
    upd_counter: AtomicU64,
    subscribers: RwLock<HashMap<String, Subscriber>>,
    storage: Arc<TimStorage>,
}

fn event_new_message(upd_id: u64, message: &Message) -> SpaceEvent {
    SpaceEvent {
        metadata: event_metadata(upd_id),
        data: Some(EventData::EventNewMessage(EventNewMessage {
            message: Some(message.clone()),
        })),
    }
}

fn event_call_ability_outcome(upd_id: u64, outcome: &CallAbilityOutcome) -> SpaceEvent {
    SpaceEvent {
        metadata: event_metadata(upd_id),
        data: Some(EventData::EventCallAbilityOutcome(
            EventCallAbilityOutcome {
                call_ability_outcome: Some(outcome.clone()),
            },
        )),
    }
}

fn event_call_ability(upd_id: u64, call_ability: &CallAbility) -> SpaceEvent {
    SpaceEvent {
        metadata: event_metadata(upd_id),
        data: Some(EventData::EventCallAbility(EventCallAbility {
            call_ability: Some(call_ability.clone()),
        })),
    }
}

fn event_timite_connected(upd_id: u64, timite: &Timite) -> SpaceEvent {
    SpaceEvent {
        metadata: event_metadata(upd_id),
        data: Some(EventData::EventTimiteConnected(EventTimiteConnected {
            timite: Some(timite.clone()),
        })),
    }
}

fn event_timite_disconnected(upd_id: u64, timite: &Timite) -> SpaceEvent {
    SpaceEvent {
        metadata: event_metadata(upd_id),
        data: Some(EventData::EventTimiteDisconnected(
            EventTimiteDisconnected {
                timite: Some(timite.clone()),
            },
        )),
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
    pub fn new(storage: Arc<TimStorage>) -> Result<TimSpace, TimSpaceError> {
        let max_event_id = storage.fetch_max_event_id()?;
        Ok(TimSpace {
            upd_counter: AtomicU64::new(max_event_id),
            subscribers: RwLock::new(HashMap::new()),
            storage,
        })
    }

    pub async fn publish_message(&self, message: &Message) -> Result<(), TimSpaceError> {
        let upd_id = self.upd_counter.fetch_add(1, Ordering::Relaxed);
        let event = event_new_message(upd_id, message);
        self.storage.store_space_event(&event)?;

        let disconnected = self
            .broadcast_event(&event, Some(message.sender_id))
            .await?;
        let removed = self.prune_disconnected(disconnected);
        self.publish_disconnected_batch(removed).await
    }

    pub async fn subscribe(
        &self,
        req: &SubscribeToSpaceReq,
        session: &Session,
        timite: Timite,
    ) -> Result<mpsc::Receiver<SpaceEvent>, TimSpaceError> {
        let (sender, receiver) = mpsc::channel(BUFFER_SIZE);
        let was_present = {
            let mut guard = self
                .subscribers
                .write()
                .expect("space events subscribers lock poisoned");
            guard.retain(|_, sub| !sub.chan.is_closed());
            let present = guard
                .values()
                .any(|subscriber| subscriber.timite.id == timite.id);
            guard.insert(
                session.key.clone(),
                Subscriber {
                    receive_own_messages: req.receive_own_messages,
                    chan: sender,
                    session: session.clone(),
                    timite: timite.clone(),
                },
            );
            present
        };

        if !was_present {
            self.publish_timite_connected(&timite).await?;
        }

        Ok(receiver)
    }

    pub async fn publish_call_outcome(
        &self,
        outcome: &CallAbilityOutcome,
        sender_timite_id: u64,
    ) -> Result<(), TimSpaceError> {
        let upd_id = self.upd_counter.fetch_add(1, Ordering::Relaxed);
        let event = event_call_ability_outcome(upd_id, outcome);
        self.storage.store_space_event(&event)?;

        let disconnected = self.broadcast_event(&event, Some(sender_timite_id)).await?;
        let removed = self.prune_disconnected(disconnected);
        self.publish_disconnected_batch(removed).await
    }

    pub async fn publish_call_ability(
        &self,
        call_ability: &CallAbility,
    ) -> Result<(), TimSpaceError> {
        let upd_id = self.upd_counter.fetch_add(1, Ordering::Relaxed);
        let event = event_call_ability(upd_id, call_ability);
        self.storage.store_space_event(&event)?;

        let disconnected = self.broadcast_event(&event, None).await?;
        let removed = self.prune_disconnected(disconnected);
        self.publish_disconnected_batch(removed).await
    }

    pub fn timeline(&self, offset: u64, size: u32) -> Result<Vec<SpaceEvent>, TimSpaceError> {
        self.storage.timeline(offset, size).map_err(Into::into)
    }

    /// Periodic cleanup task that removes all disconnected subscribers
    pub async fn cleanup_disconnected(&self) -> Result<usize, TimSpaceError> {
        let closed: Vec<Subscriber> = self
            .subscriber_snapshot()
            .into_iter()
            .filter(|sub| sub.chan.is_closed())
            .collect();
        let removed = closed.len();
        let removed_timites = self.prune_disconnected(closed);
        self.publish_disconnected_batch(removed_timites).await?;
        Ok(removed)
    }

    fn subscriber_snapshot(&self) -> Vec<Subscriber> {
        let guard = self
            .subscribers
            .read()
            .expect("space events subscribers lock poisoned");
        guard.iter().map(|(_, entry)| entry.clone()).collect()
    }

    async fn publish_timite_connected(&self, timite: &Timite) -> Result<(), TimSpaceError> {
        let upd_id = self.upd_counter.fetch_add(1, Ordering::Relaxed);
        let event = event_timite_connected(upd_id, timite);
        self.storage.store_space_event(&event)?;
        let disconnected = self.broadcast_event(&event, None).await?;
        let removed = self.prune_disconnected(disconnected);
        self.publish_disconnected_batch(removed).await
    }

    async fn publish_timite_disconnected(&self, timite: &Timite) -> Result<(), TimSpaceError> {
        let upd_id = self.upd_counter.fetch_add(1, Ordering::Relaxed);
        let event = event_timite_disconnected(upd_id, timite);
        self.storage.store_space_event(&event)?;
        let disconnected = self.broadcast_event(&event, None).await?;
        let _ = self.prune_disconnected(disconnected);
        Ok(())
    }

    fn prune_disconnected(&self, disconnected: Vec<Subscriber>) -> Vec<Timite> {
        if disconnected.is_empty() {
            return Vec::new();
        }

        let mut guard = self
            .subscribers
            .write()
            .expect("space events subscribers lock poisoned");

        let mut removed_timites = Vec::new();
        let mut seen = HashSet::new();
        for sub in disconnected {
            let removed = guard.remove(&sub.session.key);
            if removed.is_none() {
                continue;
            }
            if seen.insert(sub.timite.id)
                && !guard
                    .values()
                    .any(|candidate| candidate.timite.id == sub.timite.id)
            {
                removed_timites.push(sub.timite.clone());
            }
        }

        removed_timites
    }

    async fn broadcast_event(
        &self,
        event: &SpaceEvent,
        skip_sender: Option<u64>,
    ) -> Result<Vec<Subscriber>, TimSpaceError> {
        let snapshot = self.subscriber_snapshot();
        let mut disconnected = Vec::new();
        for sub in snapshot {
            if let Some(sender_id) = skip_sender {
                if !sub.receive_own_messages && sub.session.timite_id == sender_id {
                    continue;
                }
            }
            if sub.chan.is_closed() || sub.chan.send(event.clone()).await.is_err() {
                disconnected.push(sub);
            }
        }
        Ok(disconnected)
    }

    async fn publish_disconnected_batch(&self, removed: Vec<Timite>) -> Result<(), TimSpaceError> {
        for timite in removed {
            self.publish_timite_disconnected(&timite).await?;
        }
        Ok(())
    }
}
