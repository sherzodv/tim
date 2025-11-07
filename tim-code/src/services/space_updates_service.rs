use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::Status;

use crate::api::SpaceUpdate;
use crate::flows::update_decide_reciever_flow::{
    SpaceSubscriber, SpaceUpdateReceiverFlow, UpdateDecideReceiverFlow,
};

/// Errors returned when publishing updates into the dispatcher fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpaceUpdatesPublishError {
    /// The underlying dispatcher is no longer able to accept updates.
    DispatcherClosed,
}

impl std::fmt::Display for SpaceUpdatesPublishError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SpaceUpdatesPublishError::DispatcherClosed => {
                write!(f, "space updates dispatcher is closed")
            }
        }
    }
}

impl std::error::Error for SpaceUpdatesPublishError {}

#[async_trait::async_trait]
pub trait SpaceUpdatesService: Send + Sync {
    /// Publishes the given update to all eligible subscribers.
    async fn publish(&self, update: SpaceUpdate) -> Result<(), SpaceUpdatesPublishError>;

    /// Registers a subscriber and returns its update stream.
    fn subscribe(&self, subscriber: SpaceSubscriber)
        -> ReceiverStream<Result<SpaceUpdate, Status>>;

    /// Removes a subscriber so it no longer receives updates.
    fn unsubscribe(&self, client_id: &str);

    /// Returns true if the client currently has an active subscription.
    fn has_subscriber(&self, client_id: &str) -> bool;
}

pub struct InMemorySpaceUpdatesService {
    decider: Arc<dyn UpdateDecideReceiverFlow>,
    subscribers: RwLock<HashMap<String, SubscriberEntry>>,
    buffer_size: usize,
}

impl InMemorySpaceUpdatesService {
    pub fn new(decider: Arc<dyn UpdateDecideReceiverFlow>, buffer_size: usize) -> Self {
        Self {
            decider,
            subscribers: RwLock::new(HashMap::new()),
            buffer_size,
        }
    }

    pub fn with_default_decider(buffer_size: usize) -> Self {
        Self::new(Arc::new(SpaceUpdateReceiverFlow::new()), buffer_size)
    }
}

struct SubscriberEntry {
    sender: mpsc::Sender<Result<SpaceUpdate, Status>>,
    subscriber: SpaceSubscriber,
}

#[async_trait::async_trait]
impl SpaceUpdatesService for InMemorySpaceUpdatesService {
    async fn publish(&self, update: SpaceUpdate) -> Result<(), SpaceUpdatesPublishError> {
        let snapshot = {
            let guard = self
                .subscribers
                .read()
                .expect("space updates subscribers lock poisoned");
            guard
                .iter()
                .map(|(id, entry)| (id.clone(), entry.sender.clone(), entry.subscriber.clone()))
                .collect::<Vec<_>>()
        };

        let mut had_error = false;
        for (client_id, sender, subscriber) in snapshot {
            if !self.decider.should_deliver(&subscriber, &update) {
                continue;
            }

            if sender.send(Ok(update.clone())).await.is_err() {
                self.unsubscribe(&client_id);
                had_error = true;
            }
        }

        if had_error {
            Err(SpaceUpdatesPublishError::DispatcherClosed)
        } else {
            Ok(())
        }
    }

    fn subscribe(
        &self,
        subscriber: SpaceSubscriber,
    ) -> ReceiverStream<Result<SpaceUpdate, Status>> {
        let (sender, receiver) = mpsc::channel(self.buffer_size);
        let mut guard = self
            .subscribers
            .write()
            .expect("space updates subscribers lock poisoned");

        guard.insert(
            subscriber.client_id.clone(),
            SubscriberEntry { sender, subscriber },
        );

        ReceiverStream::new(receiver)
    }

    fn unsubscribe(&self, client_id: &str) {
        let mut guard = self
            .subscribers
            .write()
            .expect("space updates subscribers lock poisoned");
        guard.remove(client_id);
    }

    fn has_subscriber(&self, client_id: &str) -> bool {
        let guard = self
            .subscribers
            .read()
            .expect("space updates subscribers lock poisoned");
        guard.contains_key(client_id)
    }
}
