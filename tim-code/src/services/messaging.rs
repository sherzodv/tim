use crate::api::space_update::Event as SpaceUpdateEvent;
use crate::api::{Message, SpaceNewMessage, SpaceUpdate};

pub struct SessionUpdates;

impl SessionUpdates {
    pub fn message(id: String, sender_id: &str, content: impl Into<String>) -> SpaceUpdate {
        SpaceUpdate {
            id,
            event: Some(SpaceUpdateEvent::SpaceNewMessage(SpaceNewMessage {
                message: Some(Message {
                    sender_id: sender_id.to_string(),
                    content: content.into(),
                }),
            })),
        }
    }
}
