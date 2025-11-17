use std::collections::VecDeque;

use thiserror::Error;

#[derive(Debug)]
pub(super) enum LlmMemoryItem {
    Peer(String),
    Agent(String),
}

impl LlmMemoryItem {
    fn role(&self) -> &'static str {
        match self {
            LlmMemoryItem::Peer(_) => "Peer",
            LlmMemoryItem::Agent(_) => "Agent",
        }
    }

    fn content(&self) -> &str {
        match self {
            LlmMemoryItem::Peer(content) | LlmMemoryItem::Agent(content) => content,
        }
    }
}

pub(super) struct LlmMemory {
    limit: usize,
    items: VecDeque<LlmMemoryItem>,
}

#[derive(Debug, Error)]
pub(super) enum LlmMemoryError {
    #[error("memory entry is empty")]
    EmptyEntry,
}

impl LlmMemory {
    pub(super) fn new(limit: usize) -> Self {
        Self {
            limit,
            items: VecDeque::with_capacity(limit),
        }
    }

    pub(super) fn push_peer(&mut self, content: &str) -> Result<(), LlmMemoryError> {
        self.push(LlmMemoryItem::Peer(Self::normalize(content)?));
        Ok(())
    }

    pub(super) fn push_agent(&mut self, content: &str) -> Result<(), LlmMemoryError> {
        self.push(LlmMemoryItem::Agent(Self::normalize(content)?));
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

    fn push(&mut self, entry: LlmMemoryItem) {
        if self.limit == 0 {
            return;
        }
        if self.items.len() == self.limit {
            self.items.pop_front();
        }
        self.items.push_back(entry);
    }

    fn normalize(raw: &str) -> Result<String, LlmMemoryError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(LlmMemoryError::EmptyEntry);
        }
        Ok(trimmed.to_string())
    }
}
