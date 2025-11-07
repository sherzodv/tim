use crate::api::{space_update::Event, SpaceUpdate};

/// Metadata for a subscriber connection.
#[derive(Clone, Debug)]
pub struct SpaceSubscriber {
    pub client_id: String,
    pub timite_id: String,
    pub receive_own_messages: bool,
}

pub trait UpdateDecideReceiverFlow: Send + Sync {
    fn should_deliver(&self, subscriber: &SpaceSubscriber, update: &SpaceUpdate) -> bool;
}

#[derive(Default)]
pub struct SpaceUpdateReceiverFlow;

impl SpaceUpdateReceiverFlow {
    pub fn new() -> Self {
        Self
    }
}

impl UpdateDecideReceiverFlow for SpaceUpdateReceiverFlow {
    fn should_deliver(&self, subscriber: &SpaceSubscriber, update: &SpaceUpdate) -> bool {
        if subscriber.receive_own_messages {
            return true;
        }

        let Some(event) = &update.event else {
            return true;
        };

        match event {
            Event::SpaceNewMessage(space_message) => match space_message.message.as_ref() {
                Some(message) => message.sender_id != subscriber.timite_id,
                None => true,
            },
            _ => true,
        }
    }
}
