use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::time::sleep;
use tonic::{Request, Response, Status};
use tracing::{debug, info, warn};

use crate::api::tim_api_server::TimApi;
use crate::api::{SendMessageReq, SendMessageRes, SpaceUpdate, SubscribeToSpaceReq};
use crate::flows::update_decide_reciever_flow::SpaceSubscriber;

use super::assistant::{ChatBridge, ChatBridgeInitError, ChatBridgeReply};
use super::messaging::{SessionUpdates, ASSISTANT_SENDER_ID, SYSTEM_SENDER_ID};
use super::space_updates_service::{InMemorySpaceUpdatesService, SpaceUpdatesService};

const BASE_DELAY_MILLIS: u64 = 120;
const SPACE_UPDATES_BUFFER: usize = 32;

#[derive(Clone)]
pub struct TimApiService {
    space_updates: Arc<dyn SpaceUpdatesService>,
    event_counter: Arc<AtomicU64>,
    chat_bridge: Option<Arc<ChatBridge>>,
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
        let chat_bridge = match ChatBridge::from_env() {
            Ok(bridge) => {
                info!(
                    "ChatGPT integration enabled with model `{}`.",
                    bridge.model_name()
                );
                Some(Arc::new(bridge))
            }
            Err(ChatBridgeInitError::MissingApiKey) => {
                debug!(
                    "ChatGPT integration disabled: set OPENAI_TIM_API_KEY to enable assistant responses."
                );
                None
            }
            Err(ChatBridgeInitError::Client(err)) => {
                warn!("ChatGPT integration disabled: {err}");
                None
            }
        };

        Self {
            space_updates,
            event_counter: Arc::new(AtomicU64::new(0)),
            chat_bridge,
        }
    }

    async fn process_message(&self, client_id: String, request_id: String, message: String) {
        let messenger = SpaceMessenger::new(self, &request_id);
        messenger
            .push_message(client_id.as_str(), message.clone(), 0.0)
            .await;

        if let Some(bridge) = self.chat_bridge() {
            match bridge.send(&message).await {
                Ok(reply) => self.deliver_assistant_reply(&messenger, reply).await,
                Err(err) => {
                    let notice = format!("Assistant request failed: {err}.");
                    messenger.push_message(SYSTEM_SENDER_ID, notice, 1.0).await;
                }
            }
        } else {
            let notice = "Assistant is not configured. Set OPENAI_TIM_API_KEY to enable responses.";
            messenger.push_message(SYSTEM_SENDER_ID, notice, 1.0).await;
        }
    }

    async fn deliver_assistant_reply(
        &self,
        messenger: &SpaceMessenger<'_>,
        reply: ChatBridgeReply,
    ) {
        let ChatBridgeReply {
            text,
            usage,
            provider_request_id,
        } = reply;

        if let Some(request_ref) = provider_request_id {
            debug!("assistant request completed: {request_ref}");
        }

        if let Some(usage) = usage {
            debug!(
                "assistant usage: prompt={} completion={} total={}",
                usage.prompt_tokens, usage.completion_tokens, usage.total_tokens
            );
        }

        messenger.push_message(ASSISTANT_SENDER_ID, text, 1.0).await;
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

    fn chat_bridge(&self) -> Option<Arc<ChatBridge>> {
        self.chat_bridge.clone()
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
