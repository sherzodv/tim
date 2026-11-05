use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::api::Mention;
use crate::api::Message;
use crate::api::SendMessageReq;
use crate::api::Session;
use crate::tim_space::TimSpace;
use crate::tim_space::TimSpaceError;
use crate::tim_storage::TimStorage;
use crate::tim_storage::TimStorageError;

#[derive(Debug, thiserror::Error)]
pub enum TimMessageError {
    #[error("Storage error")]
    StorageError(#[from] TimStorageError),

    #[error("Space error")]
    SpaceError(#[from] TimSpaceError),

    #[error("Message {0} not found")]
    MessageMissing(u64),
}

pub struct TimMessage {
    t_store: Arc<TimStorage>,
    t_space: Arc<TimSpace>,
    msg_counter: AtomicU64,
}

impl TimMessage {
    pub fn new(t_store: Arc<TimStorage>, t_space: Arc<TimSpace>) -> Result<Self, TimMessageError> {
        let max_msg_id = t_store.fetch_max_message_id()?;
        Ok(Self {
            t_store,
            t_space,
            msg_counter: AtomicU64::new(max_msg_id),
        })
    }

    pub async fn process_message(
        &self,
        req: &SendMessageReq,
        session: &Session,
    ) -> Result<u64, TimMessageError> {
        self.t_space.validate_branch(req.branch_id)?;
        let msg_id = self.msg_counter.fetch_add(1, Ordering::Relaxed) + 1;
        let message = Message {
            id: msg_id,
            sender_id: session.timite_id,
            content: req.content.to_string(),
            mentions: self.resolve_mentions(&req.content)?,
        };
        self.t_store.store_message(msg_id, &message)?;
        self.t_space.publish_message(&message, req.branch_id)?;
        Ok(msg_id)
    }

    /// Every `@username` in the text that names an existing timite, once each, in order.
    fn resolve_mentions(&self, content: &str) -> Result<Vec<Mention>, TimMessageError> {
        let mut mentions: Vec<Mention> = Vec::new();
        for username in mention_candidates(content) {
            if mentions.iter().any(|mention| mention.username == username) {
                continue;
            }
            if let Some(timite) = self.t_store.fetch_timite_by_username(username)? {
                mentions.push(Mention {
                    timite_id: timite.id,
                    username: timite.username,
                });
            }
        }
        Ok(mentions)
    }

    pub fn find_message(&self, msg_id: u64) -> Result<Message, TimMessageError> {
        self.t_store
            .fetch_message(msg_id)?
            .ok_or(TimMessageError::MessageMissing(msg_id))
    }
}

/// Usernames written as `@username` at a word boundary.
fn mention_candidates(content: &str) -> impl Iterator<Item = &str> {
    content.match_indices('@').filter_map(move |(at, _)| {
        let preceded_by_word = content[..at]
            .chars()
            .next_back()
            .is_some_and(is_username_char);
        if preceded_by_word {
            return None;
        }
        let rest = &content[at + 1..];
        let end = rest.find(|c| !is_username_char(c)).unwrap_or(rest.len());
        (end > 0).then_some(&rest[..end])
    })
}

fn is_username_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}
