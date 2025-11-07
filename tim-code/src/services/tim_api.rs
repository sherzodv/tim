use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::time::sleep;
use tonic::{Request, Response, Status};
use tracing::warn;

use crate::api::tim_api_server::TimApi;
use crate::api::{SendMessageReq, SendMessageRes, SpaceUpdate, SubscribeToSpaceReq};
use crate::flows::update_decide_reciever_flow::SpaceSubscriber;

use super::messaging::SessionUpdates;
use super::space_updates_service::{InMemorySpaceUpdatesService, SpaceUpdatesService};

const BASE_DELAY_MILLIS: u64 = 120;
const SPACE_UPDATES_BUFFER: usize = 32;

#[derive(Clone)]
pub struct TimApiService {
    space_updates: Arc<dyn SpaceUpdatesService>,
    event_counter: Arc<AtomicU64>,
}

#[tonic::async_trait]
impl TimApi for TimApiService {
    type SubscribeToSpaceStream =
        Pin<Box<dyn tokio_stream::Stream<Item = Result<SpaceUpdate, Status>> + Send>>;

    async fn send_message(
        &self,
        request: Request<SendMessageReq>,
    ) -> Result<Response<SendMessageRes>, Status> {
        let payload = request.into_inner();
        let client_id = payload.client_id.trim();
        if client_id.is_empty() {
            return Err(Status::invalid_argument("client_id is required"));
        }

        let command = payload.command.trim();
        if command.is_empty() {
            return Ok(Response::new(SendMessageRes { id: payload.id }));
        }

        if !self.space_updates.has_subscriber(client_id) {
            return Err(Status::failed_precondition("client not subscribed"));
        }

        self.process_message(
            client_id.to_string(),
            payload.id.clone(),
            command.to_string(),
        )
        .await;

        Ok(Response::new(SendMessageRes { id: payload.id }))
    }

    async fn subscribe_to_space(
        &self,
        request: Request<SubscribeToSpaceReq>,
    ) -> Result<Response<Self::SubscribeToSpaceStream>, Status> {
        let payload = request.into_inner();
        let client_id = payload.client_id.trim();
        if client_id.is_empty() {
            return Err(Status::invalid_argument("client_id is required"));
        }

        let stream = self.space_updates.subscribe(SpaceSubscriber {
            client_id: client_id.to_string(),
            timite_id: client_id.to_string(),
            receive_own_messages: true,
        });
        Ok(Response::new(
            Box::pin(stream) as Self::SubscribeToSpaceStream
        ))
    }
}

impl TimApiService {
    pub fn new() -> Self {
        let space_updates: Arc<dyn SpaceUpdatesService> = Arc::new(
            InMemorySpaceUpdatesService::with_default_decider(SPACE_UPDATES_BUFFER),
        );

        Self {
            space_updates,
            event_counter: Arc::new(AtomicU64::new(0)),
        }
    }

    async fn process_message(&self, client_id: String, request_id: String, message: String) {
        let messenger = SpaceMessenger::new(self, &request_id);
        messenger
            .push_message(client_id.as_str(), message, 0.0)
            .await;
    }

    async fn dispatch_update(&self, update: SpaceUpdate, delay_multiplier: f64) {
        let delay =
            Duration::from_millis((BASE_DELAY_MILLIS as f64 * delay_multiplier).round() as u64);
        let updates = self.space_updates.clone();
        tokio::spawn(async move {
            if delay > Duration::from_millis(0) {
                sleep(delay).await;
            }
            if let Err(err) = updates.publish(update).await {
                warn!("failed to deliver space update: {err}");
            }
        });
    }

    fn next_event_id(&self, seed: &str) -> String {
        let value = self.event_counter.fetch_add(1, Ordering::Relaxed);
        format!("{seed}:{value}")
    }
}

struct SpaceMessenger<'a> {
    service: &'a TimApiService,
    request_id: &'a str,
}

impl<'a> SpaceMessenger<'a> {
    fn new(service: &'a TimApiService, request_id: &'a str) -> Self {
        Self {
            service,
            request_id,
        }
    }

    async fn push_message(&self, sender_id: &str, content: impl Into<String>, delay: f64) {
        let update = SessionUpdates::message(self.next_event_id(), sender_id, content);
        self.publish(update, delay).await;
    }

    async fn publish(&self, update: SpaceUpdate, delay: f64) {
        self.service.dispatch_update(update, delay).await;
    }

    fn next_event_id(&self) -> String {
        self.service.next_event_id(self.request_id)
    }
}
