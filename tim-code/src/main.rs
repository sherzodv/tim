use std::{
	net::SocketAddr,
	sync::{
		atomic::{AtomicU64, Ordering},
		Arc,
	},
	time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
	extract::ws::{Message, WebSocket, WebSocketUpgrade},
	response::Response,
	routing::get,
	Json, Router,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tracing::{error, info, warn};

const BASE_DELAY_MILLIS: u64 = 120;
const DEFAULT_STATUS: &str = "Ready";
const DEFAULT_HELP: &str =
	"Type `HELP` for available commands. Press `Esc` to cancel current input.";

#[tokio::main]
async fn main() {
	init_tracing();

	let app = Router::new().route("/health", get(health)).route("/ws", get(ws_handler));

	let port: u16 = std::env::var("TIM_CODE_PORT")
		.ok()
		.and_then(|value| value.parse().ok())
		.unwrap_or(8787);
	let host = std::env::var("TIM_CODE_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());

	let addr: SocketAddr = format!("{host}:{port}")
		.parse()
		.expect("invalid TIM_CODE_HOST or TIM_CODE_PORT");

	info!("Starting tim-code backend on {addr}");

	axum::serve(tokio::net::TcpListener::bind(addr).await.unwrap(), app)
		.await
		.unwrap();
}

fn init_tracing() {
	let default_filter = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());
	tracing_subscriber::fmt()
		.with_env_filter(tracing_subscriber::EnvFilter::new(default_filter))
		.with_target(false)
		.init();
}

async fn health() -> Json<&'static str> {
	Json("ok")
}

async fn ws_handler(ws: WebSocketUpgrade) -> Response {
	ws.on_upgrade(handle_socket)
}

async fn handle_socket(stream: WebSocket) {
	let (sender, mut receiver) = stream.split();
	let sender = Arc::new(Mutex::new(sender));
	let context = Arc::new(ClientContext::default());

	while let Some(result) = receiver.next().await {
		match result {
			Ok(Message::Text(text)) => {
				let sender = sender.clone();
				let context = context.clone();
				tokio::spawn(async move {
					if let Err(err) = process_client_message(sender, context, text).await {
						error!("Failed to process message: {err}");
					}
				});
			}
			Ok(Message::Close(_)) => break,
			Ok(other) => {
				warn!("Ignoring unsupported WebSocket message: {other:?}");
			}
			Err(err) => {
				warn!("WebSocket error: {err}");
				break;
			}
		}
	}
}

async fn process_client_message(
	sender: Arc<Mutex<SplitWebSocketSender>>,
	context: Arc<ClientContext>,
	text: String,
) -> Result<(), String> {
	let message: ClientMessage =
		serde_json::from_str(&text).map_err(|err| format!("invalid payload: {err}"))?;

	match message {
		ClientMessage::CommandRequest { id, payload } => {
			let command = payload.command.trim();
			if command.is_empty() {
				return Ok(());
			}
			handle_command(sender, context, id, command.to_string()).await;
		}
	}
	Ok(())
}

type SplitWebSocketSender = futures_util::stream::SplitSink<WebSocket, Message>;

async fn handle_command(
	sender: Arc<Mutex<SplitWebSocketSender>>,
	context: Arc<ClientContext>,
	request_id: String,
	command: String,
) {
	let mut parts = command.split_whitespace();
	let Some(keyword) = parts.next() else {
		return;
	};

	let normalized = keyword.to_ascii_lowercase();

	match normalized.as_str() {
		"help" => handle_help(sender, context, request_id.clone(), command).await,
		"clear" => handle_clear(sender, context, request_id.clone(), command).await,
		"theme" => {
			let desired = parts.next().unwrap_or_default().to_ascii_lowercase();
			handle_theme(sender, context, request_id.clone(), command, desired).await;
		}
		"reset" => handle_reset(sender, context, request_id.clone(), command).await,
		_ => handle_unknown(sender, context, request_id.clone(), command).await,
	}
}

async fn handle_help(
	sender: Arc<Mutex<SplitWebSocketSender>>,
	context: Arc<ClientContext>,
	request_id: String,
	command: String,
) {
	enqueue_message(
		sender.clone(),
		create_append_entry_message(context.next_event_id(&request_id), command_entry(command)),
		0.0,
	)
	.await;

	enqueue_message(
		sender.clone(),
		create_append_entry_message(
			context.next_event_id(&request_id),
			output_entry_html(help_html()),
		),
		1.0,
	)
	.await;

	enqueue_message(
		sender.clone(),
		create_status_message(context.next_event_id(&request_id), "Help displayed"),
		1.5,
	)
	.await;

	enqueue_message(
		sender,
		create_help_message(context.next_event_id(&request_id), DEFAULT_HELP),
		1.6,
	)
	.await;
}

async fn handle_clear(
	sender: Arc<Mutex<SplitWebSocketSender>>,
	context: Arc<ClientContext>,
	request_id: String,
	command: String,
) {
	enqueue_message(
		sender.clone(),
		ServerMessage::WorkspaceEntriesClear {
			id: context.next_event_id(&request_id),
		},
		0.2,
	)
	.await;

	enqueue_message(
		sender.clone(),
		create_append_entry_message(context.next_event_id(&request_id), command_entry(command)),
		0.4,
	)
	.await;

	enqueue_message(
		sender.clone(),
		create_append_entry_message(
			context.next_event_id(&request_id),
			output_entry_text("Workspace cleared."),
		),
		0.8,
	)
	.await;

	enqueue_message(
		sender.clone(),
		create_status_message(context.next_event_id(&request_id), "Workspace cleared"),
		1.1,
	)
	.await;

	enqueue_message(
		sender,
		create_help_message(context.next_event_id(&request_id), DEFAULT_HELP),
		1.2,
	)
	.await;
}

async fn handle_theme(
	sender: Arc<Mutex<SplitWebSocketSender>>,
	context: Arc<ClientContext>,
	request_id: String,
	command: String,
	desired: String,
) {
	enqueue_message(
		sender.clone(),
		create_append_entry_message(context.next_event_id(&request_id), command_entry(command)),
		0.0,
	)
	.await;

	match desired.as_str() {
		"night" | "day" => {
			let confirmation = format!("Theme set to {desired}.");
			enqueue_message(
				sender.clone(),
				create_append_entry_message(
					context.next_event_id(&request_id),
					output_entry_text(&confirmation),
				),
				1.0,
			)
			.await;

			enqueue_message(
				sender.clone(),
				create_theme_message(
					context.next_event_id(&request_id),
					match desired.as_str() {
						"day" => Theme::Day,
						_ => Theme::Night,
					},
				),
				1.2,
			)
			.await;

			enqueue_message(
				sender.clone(),
				create_status_message(context.next_event_id(&request_id), &confirmation),
				1.3,
			)
			.await;

			enqueue_message(
				sender,
				create_help_message(context.next_event_id(&request_id), DEFAULT_HELP),
				1.4,
			)
			.await;
		}
		_ => {
			enqueue_message(
				sender.clone(),
				create_append_entry_message(
					context.next_event_id(&request_id),
					output_entry_text("Usage: THEME <night|day>"),
				),
				1.0,
			)
			.await;

			enqueue_message(
				sender.clone(),
				create_status_message(context.next_event_id(&request_id), "Theme command incomplete"),
				1.3,
			)
			.await;

			enqueue_message(
				sender,
				create_help_message(
					context.next_event_id(&request_id),
					"Try THEME night or THEME day.",
				),
				1.4,
			)
			.await;
		}
	}
}

async fn handle_reset(
	sender: Arc<Mutex<SplitWebSocketSender>>,
	context: Arc<ClientContext>,
	request_id: String,
	command: String,
) {
	enqueue_message(
		sender.clone(),
		create_append_entry_message(context.next_event_id(&request_id), command_entry(command)),
		0.0,
	)
	.await;

	enqueue_message(
		sender.clone(),
		ServerMessage::WorkspaceEntriesClear {
			id: context.next_event_id(&request_id),
		},
		0.8,
	)
	.await;

	enqueue_message(
		sender.clone(),
		create_theme_message(
			context.next_event_id(&request_id),
			Theme::Night,
		),
		1.0,
	)
	.await;

	enqueue_message(
		sender.clone(),
		create_status_message(context.next_event_id(&request_id), DEFAULT_STATUS),
		1.1,
	)
	.await;

	enqueue_message(
		sender,
		create_help_message(context.next_event_id(&request_id), DEFAULT_HELP),
		1.2,
	)
	.await;
}

async fn handle_unknown(
	sender: Arc<Mutex<SplitWebSocketSender>>,
	context: Arc<ClientContext>,
	request_id: String,
	command: String,
) {
	let notice = format!(
		"Unknown command \"{command}\". Type HELP to show available commands."
	);

	enqueue_message(
		sender.clone(),
		create_append_entry_message(context.next_event_id(&request_id), command_entry(command)),
		0.0,
	)
	.await;

	enqueue_message(
		sender.clone(),
		create_append_entry_message(
			context.next_event_id(&request_id),
			output_entry_text(&notice),
		),
		1.0,
	)
	.await;

	enqueue_message(
		sender.clone(),
		create_status_message(context.next_event_id(&request_id), "Unknown command"),
		1.3,
	)
	.await;

	enqueue_message(
		sender,
		create_help_message(
			context.next_event_id(&request_id),
			"Type HELP to see the command list.",
		),
		1.4,
	)
	.await;
}

async fn enqueue_message(
	sender: Arc<Mutex<SplitWebSocketSender>>,
	message: ServerMessage,
	delay_multiplier: f64,
) {
	let delay =
		Duration::from_millis((BASE_DELAY_MILLIS as f64 * delay_multiplier).round() as u64);

	tokio::spawn(async move {
		if delay.as_millis() > 0 {
			tokio::time::sleep(delay).await;
		}
		let payload = serde_json::to_string(&message).expect("serialize server message");
		let mut sender = sender.lock().await;
		if let Err(err) = sender.send(Message::Text(payload)).await {
			error!("Failed to send message: {err}");
		}
	});
}

fn create_append_entry_message(id: String, entry: CommandEntry) -> ServerMessage {
	ServerMessage::WorkspaceEntryAppend {
		id,
		payload: WorkspaceEntryAppendPayload { entry },
	}
}

fn create_status_message(id: String, status: &str) -> ServerMessage {
	ServerMessage::SessionStatus {
		id,
		payload: SessionStatusPayload {
			status: status.to_string(),
		},
	}
}

fn create_help_message(id: String, help: &str) -> ServerMessage {
	ServerMessage::SessionHelp {
		id,
		payload: SessionHelpPayload {
			help: help.to_string(),
		},
	}
}

fn create_theme_message(id: String, theme: Theme) -> ServerMessage {
	ServerMessage::SessionTheme {
		id,
		payload: SessionThemePayload { theme },
	}
}

fn command_entry(command: String) -> CommandEntry {
	CommandEntry {
		id: next_entry_id(),
		role: CommandRole::Command,
		content: CommandContent::Text { text: command },
	}
}

fn output_entry_text(text: &str) -> CommandEntry {
	CommandEntry {
		id: next_entry_id(),
		role: CommandRole::Output,
		content: CommandContent::Text {
			text: text.to_string(),
		},
	}
}

fn output_entry_html(html: String) -> CommandEntry {
	CommandEntry {
		id: next_entry_id(),
		role: CommandRole::Output,
		content: CommandContent::Html { html },
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

#[derive(Default)]
struct ClientContext {
	message_counter: AtomicU64,
}

impl ClientContext {
	fn next_event_id(&self, seed: &str) -> String {
		let value = self.message_counter.fetch_add(1, Ordering::Relaxed);
		format!("{seed}:{value}")
	}
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum ClientMessage {
	#[serde(rename = "command.request")]
	CommandRequest {
		id: String,
		payload: CommandRequestPayload,
	},
}

#[derive(Deserialize)]
struct CommandRequestPayload {
	command: String,
}

#[derive(Serialize)]
#[serde(tag = "type")]
enum ServerMessage {
	#[serde(rename = "workspace.entry.append")]
	WorkspaceEntryAppend {
		id: String,
		payload: WorkspaceEntryAppendPayload,
	},
	#[serde(rename = "workspace.entries.clear")]
	WorkspaceEntriesClear { id: String },
	#[serde(rename = "session.status")]
	SessionStatus {
		id: String,
		payload: SessionStatusPayload,
	},
	#[serde(rename = "session.help")]
	SessionHelp {
		id: String,
		payload: SessionHelpPayload,
	},
	#[serde(rename = "session.theme")]
	SessionTheme {
		id: String,
		payload: SessionThemePayload,
	},
}

#[derive(Serialize)]
struct WorkspaceEntryAppendPayload {
	entry: CommandEntry,
}

#[derive(Serialize)]
struct SessionStatusPayload {
	status: String,
}

#[derive(Serialize)]
struct SessionHelpPayload {
	help: String,
}

#[derive(Serialize)]
struct SessionThemePayload {
	theme: Theme,
}

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
enum CommandRole {
	Command,
	Output,
}

#[derive(Serialize)]
struct CommandEntry {
	id: i64,
	role: CommandRole,
	content: CommandContent,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum CommandContent {
	Text { text: String },
	Html { html: String },
}

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
enum Theme {
	Night,
	Day,
}
