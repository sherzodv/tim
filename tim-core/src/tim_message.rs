use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::Arc;

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
        let msg_id = self.msg_counter.fetch_add(1, Ordering::Relaxed) + 1;
        let message = Message {
            id: msg_id,
            sender_id: session.timite_id,
            content: req.content.to_string(),
        };
        self.t_store.store_message(msg_id, &message)?;
        self.t_space.publish_message(&message).await?;
        Ok(msg_id)
    }

    pub fn find_message(&self, msg_id: u64) -> Result<Message, TimMessageError> {
        self.t_store
            .fetch_message(msg_id)?
            .ok_or(TimMessageError::MessageMissing(msg_id))
    }
}
