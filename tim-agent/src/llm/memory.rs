use crate::tim_client::SpaceEvent;

use super::storage::{timeline_event_from_update, Storage, StorageError, TimelineEvent};
use thiserror::Error;

pub(super) struct Memory {
    limit: usize,
    timite_id: u64,
    storage: Storage,
}

#[derive(Debug, Error)]
pub(super) enum MemoryError {
    #[error("memory entry is empty")]
    EmptyEntry,

    #[error("storage error: {0}")]
    Storage(#[from] StorageError),
}

impl Memory {
    pub(super) fn new(
        limit: usize,
        storage_path: &str,
        timite_id: u64,
    ) -> Result<Self, MemoryError> {
        Ok(Self {
            limit,
            timite_id,
            storage: Storage::new(storage_path)?,
        })
    }

    pub(super) fn record_space_update(&self, update: &SpaceEvent) -> Result<(), MemoryError> {
        if let Some(mut event) = timeline_event_from_update(update) {
            if event.content.trim().is_empty() {
                return Ok(());
            }
            event.timite_id = self.timite_id;
            self.storage.store_timeline_event(self.timite_id, &event)?;
        }
        Ok(())
    }

    pub(super) fn push_agent(&self, content: &str) -> Result<(), MemoryError> {
        let normalized = Self::normalize(content)?;
        let event = TimelineEvent {
            timite_id: self.timite_id,
            header: "Agent".to_string(),
            content: normalized,
        };
        self.storage.store_timeline_event(self.timite_id, &event)?;
        Ok(())
    }

    pub(super) fn context(&self) -> Option<String> {
        if self.limit == 0 {
            return None;
        }
        let fetch_limit = usize::min(self.limit, u16::MAX as usize) as u16;
        let start_at = match self.storage.timeline_size(self.timite_id) {
            Ok(total) => total.saturating_sub(fetch_limit as u64),
            Err(_) => return None,
        };
        let events = match self.storage.timeline(self.timite_id, start_at, fetch_limit) {
            Ok(events) => events,
            Err(_) => return None,
        };
        let mut buf = String::new();
        for event in events {
            if !Self::is_conversational(&event) {
                continue;
            }
            let content = event.content.trim();
            if content.is_empty() {
                continue;
            }
            buf.push_str(event.header.trim());
            buf.push_str(": ");
            buf.push_str(content);
            buf.push('\n');
        }
        if buf.is_empty() {
            None
        } else {
            Some(buf.trim_end().to_string())
        }
    }

    fn is_conversational(event: &TimelineEvent) -> bool {
        matches!(event.header.as_str(), "Peer" | "Agent")
    }

    fn normalize(raw: &str) -> Result<String, MemoryError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(MemoryError::EmptyEntry);
        }
        Ok(trimmed.to_string())
    }
}
