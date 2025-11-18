use std::time::{SystemTime, UNIX_EPOCH};

use crate::tim_client::tim_api::{CallAbility, CallAbilityOutcome};
use crate::tim_client::{Event, EventNewMessage, SpaceEvent};
use thiserror::Error;
use tim_lib::kvstore::{KvStore, KvStoreError};

pub(super) mod agent_db {
    tonic::include_proto!("tim.agent.db.g1");
}

pub(super) use agent_db::TimelineEvent;

#[derive(Debug, Error)]
pub(super) enum StorageError {
    #[error("kv store error: {0}")]
    Store(#[from] KvStoreError),
}

pub(super) struct Storage {
    store: KvStore,
}

impl Storage {
    pub(super) fn new(path: &str) -> Result<Self, StorageError> {
        Ok(Self {
            store: KvStore::new(path)?,
        })
    }

    pub(super) fn store_timeline_event(
        &self,
        timite_id: u64,
        event: &TimelineEvent,
    ) -> Result<(), StorageError> {
        let key = key::timeline(timite_id, now_micros());
        let mut stored = event.clone();
        stored.timite_id = timite_id;
        self.store.store_log(&key, &stored)?;
        Ok(())
    }

    pub(super) fn timeline_size(&self, timite_id: u64) -> Result<u64, StorageError> {
        let entries = self
            .store
            .fetch_all_log::<TimelineEvent>(&key::timeline_prefix(timite_id))?;
        Ok(entries.len() as u64)
    }

    pub(super) fn timeline(
        &self,
        timite_id: u64,
        start: u64,
        size: u16,
    ) -> Result<Vec<TimelineEvent>, StorageError> {
        let entries = self
            .store
            .fetch_all_log::<TimelineEvent>(&key::timeline_prefix(timite_id))?;
        let start_idx = usize::min(start as usize, entries.len());
        let limit = usize::from(size);
        let slice = entries.into_iter().skip(start_idx).take(limit).collect();
        Ok(slice)
    }
}

fn now_micros() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|dur| dur.as_micros() as u64)
        .unwrap_or_default()
}

mod key {
    pub fn timeline_prefix(timite_id: u64) -> Vec<u8> {
        let mut buf = Vec::with_capacity(2 + 8 + 1);
        buf.extend_from_slice(b"t:");
        buf.extend_from_slice(&timite_id.to_be_bytes());
        buf.push(b':');
        buf
    }

    pub fn timeline(timite_id: u64, timestamp: u64) -> Vec<u8> {
        let mut buf = timeline_prefix(timite_id);
        buf.extend_from_slice(&timestamp.to_be_bytes());
        buf.push(b':');
        buf
    }
}

pub(super) fn timeline_event_from_update(update: &SpaceEvent) -> Option<TimelineEvent> {
    match &update.event {
        Some(Event::EventNewMessage(new_message)) => message_event(new_message),
        Some(Event::EventCallAbility(call)) => call.call_ability.as_ref().map(call_event),
        Some(Event::EventCallAbilityOutcome(outcome)) => outcome
            .call_ability_outcome
            .as_ref()
            .map(call_outcome_event),
        None => None,
    }
}

fn message_event(event: &EventNewMessage) -> Option<TimelineEvent> {
    let message = event.message.as_ref()?;
    let content = message.content.trim();
    if content.is_empty() {
        return None;
    }
    Some(TimelineEvent {
        timite_id: 0,
        header: "Peer".to_string(),
        content: content.to_string(),
    })
}

fn call_event(call: &CallAbility) -> TimelineEvent {
    let payload = call.payload.trim();
    TimelineEvent {
        timite_id: 0,
        header: format!("CallAbility:{}", call.name.trim()),
        content: format!("sender={} payload={}", call.sender_id, payload),
    }
}

fn call_outcome_event(outcome: &CallAbilityOutcome) -> TimelineEvent {
    let mut parts = Vec::new();
    if let Some(payload) = outcome
        .payload
        .as_ref()
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
    {
        parts.push(format!("payload={payload}"));
    }
    if let Some(error) = outcome
        .error
        .as_ref()
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
    {
        parts.push(format!("error={error}"));
    }
    let mut content = format!("call_id={}", outcome.call_ability_id);
    if !parts.is_empty() {
        content.push(' ');
        content.push_str(&parts.join(" "));
    }
    TimelineEvent {
        timite_id: 0,
        header: "CallAbilityOutcome".to_string(),
        content,
    }
}
