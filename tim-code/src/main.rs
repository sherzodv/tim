mod api {
    tonic::include_proto!("tim.api.g1");
}

pub mod gpt;

use crate::gpt::chatgpt::ChatGptClient;
use crate::gpt::{
    GptChatRequest, GptClient, GptClientError, GptGenerationControls, GptMessage, GptMessageRole,
    GptUsage,
};
use api::command_content::Value as CommandContentValue;
use api::server_message::Event as ServerMessageEvent;
use api::tim_api_server::{TimApi, TimApiServer};
use api::{
    CommandContent, CommandEntry, CommandRequest, CommandRole, SendCommandResponse, ServerMessage,
    SessionHelp, SessionStatus, SessionTheme, SubscribeRequest, Theme, WorkspaceEntriesClear,
    WorkspaceEntryAppend,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, RwLock};
use tokio::time::sleep;
use tokio_stream::{wrappers::ReceiverStream, StreamExt};
use tonic::{transport::Server, Request, Response, Status};
use tonic_web::GrpcWebLayer;
use tower_http::cors::{Any, CorsLayer};
use tracing::{debug, info, warn};

const BASE_DELAY_MILLIS: u64 = 120;
const DEFAULT_STATUS: &str = "Ready";
const DEFAULT_HELP: &str =
    "Type `HELP` for available commands. Press `Esc` to cancel current input.";

#[derive(Clone)]
struct TimApiImpl {
    clients: Arc<RwLock<HashMap<String, mpsc::Sender<ServerMessage>>>>,
    event_counter: Arc<AtomicU64>,
    chat_bridge: Option<Arc<ChatBridge>>,
}

#[tonic::async_trait]
impl TimApi for TimApiImpl {
    type SubscribeStream =
        Pin<Box<dyn tokio_stream::Stream<Item = Result<ServerMessage, Status>> + Send>>;

    async fn send_command(
        &self,
        request: Request<CommandRequest>,
    ) -> Result<Response<SendCommandResponse>, Status> {
        let payload = request.into_inner();
        let client_id = payload.client_id.trim();
        if client_id.is_empty() {
            return Err(Status::invalid_argument("client_id is required"));
        }

        let command = payload.command.trim();
        if command.is_empty() {
            return Ok(Response::new(SendCommandResponse { id: payload.id }));
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

        Ok(Response::new(SendCommandResponse { id: payload.id }))
    }

    async fn subscribe(
        &self,
        request: Request<SubscribeRequest>,
    ) -> Result<Response<Self::SubscribeStream>, Status> {
        let payload = request.into_inner();
        let client_id = payload.client_id.trim();
        if client_id.is_empty() {
            return Err(Status::invalid_argument("client_id is required"));
        }

        let (sender, receiver) = mpsc::channel(32);
        self.add_client(client_id.to_string(), sender).await;

        let stream = ReceiverStream::new(receiver).map(Ok);
        Ok(Response::new(Box::pin(stream) as Self::SubscribeStream))
    }
}

impl TimApiImpl {
    fn new() -> Self {
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

    async fn add_client(&self, id: String, sender: mpsc::Sender<ServerMessage>) {
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

    async fn process_command(&self, client_id: String, request_id: String, command: String) {
        let mut parts = command.split_whitespace();
        let Some(keyword) = parts.next() else {
            return;
        };

        let normalized = keyword.to_ascii_lowercase();

        match normalized.as_str() {
            "help" => self.handle_help(client_id, request_id, command).await,
            "clear" => self.handle_clear(client_id, request_id, command).await,
            "theme" => {
                let desired = parts.next().unwrap_or_default().to_ascii_lowercase();
                self.handle_theme(client_id, request_id, command, desired)
                    .await;
            }
            "reset" => self.handle_reset(client_id, request_id, command).await,
            _ => self.handle_unknown(client_id, request_id, command).await,
        }
    }

    async fn handle_help(&self, client_id: String, request_id: String, command: String) {
        self.enqueue_message(
            client_id.clone(),
            self.create_append_entry_message(
                self.next_event_id(&request_id),
                command_entry(command),
            ),
            0.0,
        )
        .await;

        self.enqueue_message(
            client_id.clone(),
            self.create_append_entry_message(
                self.next_event_id(&request_id),
                output_entry_html(help_html()),
            ),
            1.0,
        )
        .await;

        self.enqueue_message(
            client_id.clone(),
            self.create_status_message(self.next_event_id(&request_id), "Help displayed"),
            1.5,
        )
        .await;

        self.enqueue_message(
            client_id,
            self.create_help_message(self.next_event_id(&request_id), DEFAULT_HELP),
            1.6,
        )
        .await;
    }

    async fn handle_clear(&self, client_id: String, request_id: String, command: String) {
        self.enqueue_message(
            client_id.clone(),
            self.create_workspace_clear_message(self.next_event_id(&request_id)),
            0.2,
        )
        .await;

        self.enqueue_message(
            client_id.clone(),
            self.create_append_entry_message(
                self.next_event_id(&request_id),
                command_entry(command),
            ),
            0.4,
        )
        .await;

        self.enqueue_message(
            client_id.clone(),
            self.create_append_entry_message(
                self.next_event_id(&request_id),
                output_entry_text("Workspace cleared."),
            ),
            0.8,
        )
        .await;

        self.enqueue_message(
            client_id.clone(),
            self.create_status_message(self.next_event_id(&request_id), "Workspace cleared"),
            1.1,
        )
        .await;

        self.enqueue_message(
            client_id,
            self.create_help_message(self.next_event_id(&request_id), DEFAULT_HELP),
            1.2,
        )
        .await;
    }

    async fn handle_theme(
        &self,
        client_id: String,
        request_id: String,
        command: String,
        desired: String,
    ) {
        self.enqueue_message(
            client_id.clone(),
            self.create_append_entry_message(
                self.next_event_id(&request_id),
                command_entry(command),
            ),
            0.0,
        )
        .await;

        match desired.as_str() {
            "night" | "day" => {
                let confirmation = format!("Theme set to {desired}.");
                let theme = match desired.as_str() {
                    "day" => Theme::Day,
                    _ => Theme::Night,
                };

                self.enqueue_message(
                    client_id.clone(),
                    self.create_append_entry_message(
                        self.next_event_id(&request_id),
                        output_entry_text(&confirmation),
                    ),
                    1.0,
                )
                .await;

                self.enqueue_message(
                    client_id.clone(),
                    self.create_theme_message(self.next_event_id(&request_id), theme),
                    1.2,
                )
                .await;

                self.enqueue_message(
                    client_id.clone(),
                    self.create_status_message(self.next_event_id(&request_id), &confirmation),
                    1.3,
                )
                .await;

                self.enqueue_message(
                    client_id,
                    self.create_help_message(self.next_event_id(&request_id), DEFAULT_HELP),
                    1.4,
                )
                .await;
            }
            _ => {
                self.enqueue_message(
                    client_id.clone(),
                    self.create_append_entry_message(
                        self.next_event_id(&request_id),
                        output_entry_text("Usage: THEME <night|day>"),
                    ),
                    1.0,
                )
                .await;

                self.enqueue_message(
                    client_id.clone(),
                    self.create_status_message(
                        self.next_event_id(&request_id),
                        "Theme command incomplete",
                    ),
                    1.3,
                )
                .await;

                self.enqueue_message(
                    client_id,
                    self.create_help_message(
                        self.next_event_id(&request_id),
                        "Try THEME night or THEME day.",
                    ),
                    1.4,
                )
                .await;
            }
        };
    }

    async fn handle_reset(&self, client_id: String, request_id: String, command: String) {
        self.enqueue_message(
            client_id.clone(),
            self.create_append_entry_message(
                self.next_event_id(&request_id),
                command_entry(command),
            ),
            0.0,
        )
        .await;

        self.enqueue_message(
            client_id.clone(),
            self.create_workspace_clear_message(self.next_event_id(&request_id)),
            0.8,
        )
        .await;

        self.enqueue_message(
            client_id.clone(),
            self.create_theme_message(self.next_event_id(&request_id), Theme::Night),
            1.0,
        )
        .await;

        self.enqueue_message(
            client_id.clone(),
            self.create_status_message(self.next_event_id(&request_id), DEFAULT_STATUS),
            1.1,
        )
        .await;

        self.enqueue_message(
            client_id,
            self.create_help_message(self.next_event_id(&request_id), DEFAULT_HELP),
            1.2,
        )
        .await;
    }

    async fn handle_unknown(&self, client_id: String, request_id: String, command: String) {
        self.enqueue_message(
            client_id.clone(),
            self.create_append_entry_message(
                self.next_event_id(&request_id),
                command_entry(command.clone()),
            ),
            0.0,
        )
        .await;

        if let Some(bridge) = self.chat_bridge.clone() {
            self.enqueue_message(
                client_id.clone(),
                self.create_status_message(
                    self.next_event_id(&request_id),
                    "Consulting assistant…",
                ),
                0.2,
            )
            .await;

            match bridge.send(&command).await {
                Ok(reply) => {
                    let ChatBridgeReply {
                        text,
                        usage,
                        provider_request_id,
                    } = reply;

                    if let Some(request_ref) = provider_request_id {
                        debug!("assistant request completed: {request_ref}");
                    }

                    self.enqueue_message(
                        client_id.clone(),
                        self.create_append_entry_message(
                            self.next_event_id(&request_id),
                            output_entry_text(&text),
                        ),
                        1.0,
                    )
                    .await;

                    let status_text = usage
                        .map(|usage| {
                            format!("Assistant responded ({} tokens).", usage.total_tokens)
                        })
                        .unwrap_or_else(|| "Assistant responded.".to_string());

                    self.enqueue_message(
                        client_id.clone(),
                        self.create_status_message(self.next_event_id(&request_id), &status_text),
                        1.3,
                    )
                    .await;

                    self.enqueue_message(
                        client_id,
                        self.create_help_message(self.next_event_id(&request_id), DEFAULT_HELP),
                        1.4,
                    )
                    .await;
                }
                Err(err) => {
                    let notice = format!(
                        "Assistant request failed: {err}. Type HELP for available commands."
                    );

                    self.enqueue_message(
                        client_id.clone(),
                        self.create_append_entry_message(
                            self.next_event_id(&request_id),
                            output_entry_text(&notice),
                        ),
                        1.0,
                    )
                    .await;

                    self.enqueue_message(
                        client_id.clone(),
                        self.create_status_message(
                            self.next_event_id(&request_id),
                            "Assistant unavailable",
                        ),
                        1.3,
                    )
                    .await;

                    self.enqueue_message(
                        client_id,
                        self.create_help_message(self.next_event_id(&request_id), DEFAULT_HELP),
                        1.4,
                    )
                    .await;
                }
            }
        } else {
            let notice = "Assistant is not configured. Set OPENAI_TIM_API_KEY to enable responses.";

            self.enqueue_message(
                client_id.clone(),
                self.create_append_entry_message(
                    self.next_event_id(&request_id),
                    output_entry_text(notice),
                ),
                1.0,
            )
            .await;

            self.enqueue_message(
                client_id.clone(),
                self.create_status_message(self.next_event_id(&request_id), "Assistant disabled"),
                1.3,
            )
            .await;

            self.enqueue_message(
                client_id,
                self.create_help_message(self.next_event_id(&request_id), DEFAULT_HELP),
                1.4,
            )
            .await;
        }
    }

    async fn enqueue_message(
        &self,
        client_id: String,
        message: ServerMessage,
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

    fn create_append_entry_message(&self, id: String, entry: CommandEntry) -> ServerMessage {
        ServerMessage {
            id,
            event: Some(ServerMessageEvent::WorkspaceEntryAppend(
                WorkspaceEntryAppend { entry: Some(entry) },
            )),
        }
    }

    fn create_workspace_clear_message(&self, id: String) -> ServerMessage {
        ServerMessage {
            id,
            event: Some(ServerMessageEvent::WorkspaceEntriesClear(
                WorkspaceEntriesClear {},
            )),
        }
    }

    fn create_status_message(&self, id: String, status: &str) -> ServerMessage {
        ServerMessage {
            id,
            event: Some(ServerMessageEvent::SessionStatus(SessionStatus {
                status: status.to_string(),
            })),
        }
    }

    fn create_help_message(&self, id: String, help: &str) -> ServerMessage {
        ServerMessage {
            id,
            event: Some(ServerMessageEvent::SessionHelp(SessionHelp {
                help: help.to_string(),
            })),
        }
    }

    fn create_theme_message(&self, id: String, theme: Theme) -> ServerMessage {
        ServerMessage {
            id,
            event: Some(ServerMessageEvent::SessionTheme(SessionTheme {
                theme: theme as i32,
            })),
        }
    }
}

#[derive(Clone)]
struct ChatBridge {
    client: ChatGptClient,
    model: String,
    system_prompt: String,
    max_output_tokens: Option<u32>,
    temperature: f32,
    timeout: Duration,
}

impl ChatBridge {
    fn from_env() -> Result<Self, ChatBridgeInitError> {
        let api_key =
            std::env::var("OPENAI_TIM_API_KEY").map_err(|_| ChatBridgeInitError::MissingApiKey)?;
        let model = std::env::var("OPENAI_TIM_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string());
        let system_prompt = std::env::var("OPENAI_TIM_SYSTEM_PROMPT")
            .unwrap_or_else(|_| "You are Tim, a command centric assistant.".to_string());
        let client = ChatGptClient::new(api_key).map_err(|err| {
            ChatBridgeInitError::Client(format!("failed to create ChatGPT client: {err}"))
        })?;

        Ok(Self {
            client,
            model,
            system_prompt,
            max_output_tokens: Some(320),
            temperature: 0.2,
            timeout: Duration::from_secs(30),
        })
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    async fn send(&self, user_command: &str) -> Result<ChatBridgeReply, GptClientError> {
        let mut controls = GptGenerationControls::new();
        controls.max_output_tokens = self.max_output_tokens;
        controls.temperature = Some(self.temperature);
        controls.timeout = Some(self.timeout);

        let messages = vec![
            GptMessage {
                role: GptMessageRole::System,
                content: self.system_prompt.clone(),
            },
            GptMessage {
                role: GptMessageRole::User,
                content: user_command.to_string(),
            },
        ];

        let request = GptChatRequest::new(self.model.clone(), messages).with_controls(controls);
        let response = self.client.chat(request).await?;
        let text = response
            .choices
            .first()
            .map(|choice| choice.message.content.clone())
            .filter(|content| !content.trim().is_empty())
            .ok_or_else(|| {
                GptClientError::Provider("assistant returned an empty response".to_string())
            })?;

        Ok(ChatBridgeReply {
            text,
            usage: response.usage,
            provider_request_id: response.provider_request_id,
        })
    }
}

#[derive(Debug)]
enum ChatBridgeInitError {
    MissingApiKey,
    Client(String),
}

impl std::fmt::Display for ChatBridgeInitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChatBridgeInitError::MissingApiKey => {
                write!(f, "OPENAI_TIM_API_KEY environment variable is not set")
            }
            ChatBridgeInitError::Client(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for ChatBridgeInitError {}

struct ChatBridgeReply {
    text: String,
    usage: Option<GptUsage>,
    provider_request_id: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();

    let port: u16 = std::env::var("TIM_CODE_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(8787);
    let host = std::env::var("TIM_CODE_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());

    let addr: SocketAddr = format!("{host}:{port}")
        .parse()
        .expect("invalid TIM_CODE_HOST or TIM_CODE_PORT");

    let service = TimApiImpl::new();
    let svc = TimApiServer::new(service);
    let cors = CorsLayer::new()
        .allow_methods(Any)
        .allow_headers(Any)
        .allow_origin(Any);

    info!("Starting tim-code gRPC backend on {addr}");

    Server::builder()
        .accept_http1(true)
        .layer(cors)
        .layer(GrpcWebLayer::new())
        .add_service(svc)
        .serve(addr)
        .await?;

    Ok(())
}

fn command_entry(command: String) -> CommandEntry {
    CommandEntry {
        id: next_entry_id(),
        role: CommandRole::Command as i32,
        content: Some(CommandContent {
            value: Some(CommandContentValue::Text(command)),
        }),
    }
}

fn output_entry_text(text: &str) -> CommandEntry {
    CommandEntry {
        id: next_entry_id(),
        role: CommandRole::Output as i32,
        content: Some(CommandContent {
            value: Some(CommandContentValue::Text(text.to_string())),
        }),
    }
}

fn output_entry_html(html: String) -> CommandEntry {
    CommandEntry {
        id: next_entry_id(),
        role: CommandRole::Output as i32,
        content: Some(CommandContent {
            value: Some(CommandContentValue::Html(html)),
        }),
    }
}

fn help_html() -> String {
    r#"
<div class="help-block">
	<h3>Available commands</h3>
	<dl class="help-list">
		<dt>HELP</dt>
		<dd>Display this help overview.</dd>
		<dt>CLEAR</dt>
		<dd>Reset the workspace log.</dd>
		<dt>THEME &lt;night|day&gt;</dt>
		<dd>Switch the active theme.</dd>
	</dl>
	<p class="help-hint">Commands are case-insensitive. Try “THEME night”.</p>
</div>
"#
    .trim()
    .to_string()
}

fn next_entry_id() -> i64 {
    static ENTRY_COUNTER: AtomicU64 = AtomicU64::new(0);
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_millis() as u64;
    let counter = ENTRY_COUNTER.fetch_add(1, Ordering::Relaxed) % 1000;
    (millis * 1000 + counter) as i64
}

fn init_tracing() {
    let default_filter = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(default_filter))
        .with_target(false)
        .init();
}
