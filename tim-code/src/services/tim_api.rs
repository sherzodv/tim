use std::collections::HashMap;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, RwLock};
use tokio::time::sleep;
use tokio_stream::{wrappers::ReceiverStream, StreamExt};
use tonic::{Request, Response, Status};
use tracing::{debug, info, warn};

use crate::api::tim_api_server::TimApi;
use crate::api::{CommandEntry, SendMessageReq, SendMessageRes, SpaceUpdate, SubscribeToSpaceReq};

use super::assistant::{ChatBridge, ChatBridgeInitError, ChatBridgeReply};
use super::messaging::{
    command_entry, help_html, output_entry_html, output_entry_text, SessionUpdates,
    ASSISTANT_SENDER_ID, DEFAULT_HELP, DEFAULT_STATUS, SYSTEM_SENDER_ID,
};

const BASE_DELAY_MILLIS: u64 = 120;

#[derive(Clone)]
pub struct TimApiService {
    clients: Arc<RwLock<HashMap<String, mpsc::Sender<SpaceUpdate>>>>,
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

        if !self.client_exists(client_id).await {
            return Err(Status::failed_precondition("client not subscribed"));
        }

        self.process_command(
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

        let (sender, receiver) = mpsc::channel(32);
        self.add_client(client_id.to_string(), sender).await;

        let stream = ReceiverStream::new(receiver).map(Ok);
        Ok(Response::new(
            Box::pin(stream) as Self::SubscribeToSpaceStream
        ))
    }
}

impl TimApiService {
    pub fn new() -> Self {
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
            clients: Arc::new(RwLock::new(HashMap::new())),
            event_counter: Arc::new(AtomicU64::new(0)),
            chat_bridge,
        }
    }

    async fn process_command(&self, client_id: String, request_id: String, command: String) {
        CommandRouter::new(self.clone(), client_id, request_id, command)
            .dispatch()
            .await;
    }

    async fn add_client(&self, id: String, sender: mpsc::Sender<SpaceUpdate>) {
        let mut clients = self.clients.write().await;
        clients.insert(id, sender);
    }

    async fn client_exists(&self, id: &str) -> bool {
        let clients = self.clients.read().await;
        clients.contains_key(id)
    }

    async fn remove_client(&self, id: &str) {
        let mut clients = self.clients.write().await;
        clients.remove(id);
    }

    async fn enqueue_message(
        &self,
        client_id: String,
        message: SpaceUpdate,
        delay_multiplier: f64,
    ) {
        let delay =
            Duration::from_millis((BASE_DELAY_MILLIS as f64 * delay_multiplier).round() as u64);
        let maybe_sender = { self.clients.read().await.get(&client_id).cloned() };

        if let Some(sender) = maybe_sender {
            let service = self.clone();
            tokio::spawn(async move {
                if delay > Duration::from_millis(0) {
                    sleep(delay).await;
                }
                if sender.send(message).await.is_err() {
                    service.remove_client(&client_id).await;
                }
            });
        } else {
            warn!("client `{client_id}` missing when delivering message");
        }
    }

    fn next_event_id(&self, seed: &str) -> String {
        let value = self.event_counter.fetch_add(1, Ordering::Relaxed);
        format!("{seed}:{value}")
    }

    fn chat_bridge(&self) -> Option<Arc<ChatBridge>> {
        self.chat_bridge.clone()
    }
}

struct CommandRouter {
    service: TimApiService,
    client_id: String,
    request_id: String,
    command: String,
    keyword: String,
    args: Vec<String>,
}

impl CommandRouter {
    fn new(service: TimApiService, client_id: String, request_id: String, command: String) -> Self {
        let mut segments = command.split_whitespace();
        let keyword = segments
            .next()
            .map(|value| value.to_ascii_lowercase())
            .unwrap_or_default();
        let args = segments.map(|value| value.to_string()).collect();

        Self {
            service,
            client_id,
            request_id,
            command,
            keyword,
            args,
        }
    }

    async fn dispatch(self) {
        if self.keyword.is_empty() {
            return;
        }

        match self.keyword.as_str() {
            "help" => self.handle_help().await,
            "clear" => self.handle_clear().await,
            "theme" => {
                let desired = self
                    .args
                    .get(0)
                    .map(|value| value.to_ascii_lowercase())
                    .unwrap_or_default();
                self.handle_theme(&desired).await;
            }
            "reset" => self.handle_reset().await,
            _ => self.handle_unknown().await,
        }
    }

    async fn handle_help(&self) {
        let messenger = self.messenger();
        messenger
            .push_entry(
                self.client_id.as_str(),
                command_entry(self.command.clone()),
                0.0,
            )
            .await;

        messenger
            .push_entry(SYSTEM_SENDER_ID, output_entry_html(help_html()), 1.0)
            .await;

        messenger.push_status("Help displayed", 1.5).await;

        messenger.push_help(DEFAULT_HELP, 1.6).await;
    }

    async fn handle_clear(&self) {
        let messenger = self.messenger();
        messenger.clear_workspace(0.2).await;

        messenger
            .push_entry(
                self.client_id.as_str(),
                command_entry(self.command.clone()),
                0.4,
            )
            .await;

        messenger
            .push_entry(
                SYSTEM_SENDER_ID,
                output_entry_text("Workspace cleared."),
                0.8,
            )
            .await;

        messenger.push_status("Workspace cleared", 1.1).await;

        messenger.push_help(DEFAULT_HELP, 1.2).await;
    }

    async fn handle_theme(&self, desired: &str) {
        let messenger = self.messenger();
        messenger
            .push_entry(
                self.client_id.as_str(),
                command_entry(self.command.clone()),
                0.0,
            )
            .await;

        match desired {
            "night" | "day" => {
                let confirmation = format!("Theme set to {desired}.");
                let theme = match desired {
                    "day" => crate::api::Theme::Day,
                    _ => crate::api::Theme::Night,
                };

                messenger
                    .push_entry(SYSTEM_SENDER_ID, output_entry_text(&confirmation), 1.0)
                    .await;

                messenger.push_theme(theme, 1.2).await;

                messenger.push_status(&confirmation, 1.3).await;

                messenger.push_help(DEFAULT_HELP, 1.4).await;
            }
            _ => {
                messenger
                    .push_entry(
                        SYSTEM_SENDER_ID,
                        output_entry_text("Usage: THEME <night|day>"),
                        1.0,
                    )
                    .await;

                messenger.push_status("Theme command incomplete", 1.3).await;

                messenger
                    .push_help("Try THEME night or THEME day.", 1.4)
                    .await;
            }
        }
    }

    async fn handle_reset(&self) {
        let messenger = self.messenger();
        messenger
            .push_entry(
                self.client_id.as_str(),
                command_entry(self.command.clone()),
                0.0,
            )
            .await;

        messenger.clear_workspace(0.8).await;
        messenger.push_theme(crate::api::Theme::Night, 1.0).await;
        messenger.push_status(DEFAULT_STATUS, 1.1).await;
        messenger.push_help(DEFAULT_HELP, 1.2).await;
    }

    async fn handle_unknown(&self) {
        let messenger = self.messenger();
        messenger
            .push_entry(
                self.client_id.as_str(),
                command_entry(self.command.clone()),
                0.0,
            )
            .await;

        if let Some(bridge) = self.service.chat_bridge() {
            messenger.push_status("Consulting assistant…", 0.2).await;

            match bridge.send(&self.command).await {
                Ok(reply) => self.handle_assistant_reply(&messenger, reply).await,
                Err(err) => {
                    let notice = format!(
                        "Assistant request failed: {err}. Type HELP for available commands."
                    );

                    messenger
                        .push_entry(SYSTEM_SENDER_ID, output_entry_text(&notice), 1.0)
                        .await;

                    messenger.push_status("Assistant unavailable", 1.3).await;

                    messenger.push_help(DEFAULT_HELP, 1.4).await;
                }
            }
        } else {
            let notice = "Assistant is not configured. Set OPENAI_TIM_API_KEY to enable responses.";

            messenger
                .push_entry(SYSTEM_SENDER_ID, output_entry_text(notice), 1.0)
                .await;

            messenger.push_status("Assistant disabled", 1.3).await;

            messenger.push_help(DEFAULT_HELP, 1.4).await;
        }
    }

    async fn handle_assistant_reply(
        &self,
        messenger: &CommandMessenger<'_>,
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

        messenger
            .push_entry(ASSISTANT_SENDER_ID, output_entry_text(&text), 1.0)
            .await;

        let status_text = usage
            .map(|usage| format!("Assistant responded ({} tokens).", usage.total_tokens))
            .unwrap_or_else(|| "Assistant responded.".to_string());

        messenger.push_status(&status_text, 1.3).await;

        messenger.push_help(DEFAULT_HELP, 1.4).await;
    }

    fn messenger(&self) -> CommandMessenger<'_> {
        CommandMessenger::new(&self.service, &self.client_id, &self.request_id)
    }
}

struct CommandMessenger<'a> {
    service: &'a TimApiService,
    client_id: &'a str,
    request_id: &'a str,
}

impl<'a> CommandMessenger<'a> {
    fn new(service: &'a TimApiService, client_id: &'a str, request_id: &'a str) -> Self {
        Self {
            service,
            client_id,
            request_id,
        }
    }

    async fn push_entry(&self, sender_id: &str, entry: CommandEntry, delay: f64) {
        let update = SessionUpdates::append_entry(self.next_event_id(), sender_id, entry);
        self.publish(update, delay).await;
    }

    async fn push_status(&self, status: &str, delay: f64) {
        let update = SessionUpdates::status(self.next_event_id(), status);
        self.publish(update, delay).await;
    }

    async fn push_help(&self, help: &str, delay: f64) {
        let update = SessionUpdates::help(self.next_event_id(), help.to_string());
        self.publish(update, delay).await;
    }

    async fn push_theme(&self, theme: crate::api::Theme, delay: f64) {
        let update = SessionUpdates::theme(self.next_event_id(), theme);
        self.publish(update, delay).await;
    }

    async fn clear_workspace(&self, delay: f64) {
        let update = SessionUpdates::workspace_cleared(self.next_event_id());
        self.publish(update, delay).await;
    }

    async fn publish(&self, update: SpaceUpdate, delay: f64) {
        self.service
            .enqueue_message(self.client_id.to_string(), update, delay)
            .await;
    }

    fn next_event_id(&self) -> String {
        self.service.next_event_id(self.request_id)
    }
}
