use std::collections::VecDeque;

use thiserror::Error;

#[derive(Debug)]
pub(super) enum MemoryItem {
    Peer(String),
    Agent(String),
}

impl MemoryItem {
    fn role(&self) -> &'static str {
        match self {
            MemoryItem::Peer(_) => "Peer",
            MemoryItem::Agent(_) => "Agent",
        }
    }

    fn content(&self) -> &str {
        match self {
            MemoryItem::Peer(content) | MemoryItem::Agent(content) => content,
        }
    }
}

pub(super) struct Memory {
    limit: usize,
    items: VecDeque<MemoryItem>,
}

#[derive(Debug, Error)]
pub(super) enum MemoryError {
    #[error("memory entry is empty")]
    EmptyEntry,
}

impl Memory {
    pub(super) fn new(limit: usize) -> Self {
        Self {
            limit,
            items: VecDeque::with_capacity(limit),
        }
    }

    pub(super) fn push_peer(&mut self, content: &str) -> Result<(), MemoryError> {
        self.push(MemoryItem::Peer(Self::normalize(content)?));
        Ok(())
    }

    pub(super) fn push_agent(&mut self, content: &str) -> Result<(), MemoryError> {
        self.push(MemoryItem::Agent(Self::normalize(content)?));
        Ok(())
    }

    pub(super) fn context(&self) -> Option<String> {
        if self.items.is_empty() {
            return None;
        }
        let mut buf = String::new();
        for entry in &self.items {
            buf.push_str(entry.role());
            buf.push_str(": ");
            buf.push_str(entry.content());
            buf.push('\n');
        }
        Some(buf.trim_end().to_string())
    }

    fn push(&mut self, entry: MemoryItem) {
        if self.limit == 0 {
            return;
        }
        if self.items.len() == self.limit {
            self.items.pop_front();
        }
        self.items.push_back(entry);
    }

    fn normalize(raw: &str) -> Result<String, MemoryError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(MemoryError::EmptyEntry);
        }
        Ok(trimmed.to_string())
    }
}
