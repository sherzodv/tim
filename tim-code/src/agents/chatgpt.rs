use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use std::time::Duration;

use tokio::time::sleep;
use tonic::transport::{Channel, Endpoint};
use tracing::{debug, info, warn};

use crate::api::tim_api_client::TimApiClient;
use crate::api::{space_update::Event, SendMessageReq, SpaceUpdate, SubscribeToSpaceReq};
use crate::gpt::chatgpt::ChatGptClient;
use crate::gpt::{
    GptChatRequest, GptClient, GptClientError, GptGenerationControls, GptMessage, GptMessageRole,
};

const DEFAULT_AGENT_CLIENT_ID: &str = "agent-chatgpt";

pub fn spawn(endpoint: &str) -> bool {
    let bridge = match ChatBridge::from_env() {
        Ok(bridge) => Arc::new(bridge),
        Err(ChatBridgeInitError::MissingApiKey) => {
            debug!("ChatGPT agent disabled: set OPENAI_TIM_API_KEY to enable automated responses.");
            return false;
        }
        Err(ChatBridgeInitError::Client(err)) => {
            warn!("ChatGPT agent disabled: {err}");
            return false;
        }
    };

    let config = AgentConfig::from_env(endpoint);
    tokio::spawn(async move {
        run_agent(config, bridge).await;
    });
    true
}

async fn run_agent(config: AgentConfig, bridge: Arc<ChatBridge>) {
    let mut retry_delay = Duration::from_secs(1);
    loop {
        match connect_and_listen(&config, bridge.clone()).await {
            Ok(_) => {
                retry_delay = Duration::from_secs(1);
            }
            Err(err) => {
                warn!("chatgpt agent loop error: {err}");
                sleep(retry_delay).await;
                retry_delay = (retry_delay * 2).min(Duration::from_secs(30));
            }
        }
    }
}

async fn connect_and_listen(
    config: &AgentConfig,
    bridge: Arc<ChatBridge>,
) -> Result<(), AgentError> {
    let endpoint = Endpoint::from_shared(config.endpoint.clone())?;
    let channel = endpoint.connect().await?;
    let mut client = TimApiClient::new(channel);
    let mut stream = client
        .subscribe_to_space(SubscribeToSpaceReq {
            client_id: config.client_id.clone(),
        })
        .await?
        .into_inner();

    info!(
        "ChatGPT agent `{}` subscribed to tim-code backend.",
        config.client_id
    );

    while let Some(update) = stream.message().await? {
        if let Some((sender_id, text)) = extract_message(&update) {
            if sender_id == config.client_id || text.trim().is_empty() {
                continue;
            }

            match bridge.send(&text).await {
                Ok(reply) => {
                    send_reply(
                        &mut client,
                        &config.client_id,
                        &config.request_id_seed,
                        reply.text,
                    )
                    .await?;
                }
                Err(err) => warn!("chatgpt agent request failed: {err}"),
            }
        }
    }

    Ok(())
}

fn extract_message(update: &SpaceUpdate) -> Option<(String, String)> {
    match update.event.as_ref()? {
        Event::SpaceNewMessage(message) => {
            let message = message.message.as_ref()?;
            Some((message.sender_id.clone(), message.content.clone()))
        }
    }
}

async fn send_reply(
    client: &mut TimApiClient<Channel>,
    client_id: &str,
    seed: &str,
    message: String,
) -> Result<(), AgentError> {
    let id = next_request_id(seed);
    client
        .send_message(SendMessageReq {
            id,
            command: message,
            client_id: client_id.to_string(),
        })
        .await?;
    Ok(())
}

fn next_request_id(seed: &str) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let value = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{seed}:{value}")
}

struct AgentConfig {
    endpoint: String,
    client_id: String,
    request_id_seed: String,
}

impl AgentConfig {
    fn from_env(endpoint: &str) -> Self {
        let client_id =
            std::env::var("OPENAI_TIM_AGENT_ID").unwrap_or_else(|_| DEFAULT_AGENT_CLIENT_ID.into());
        let request_id_seed =
            std::env::var("OPENAI_TIM_AGENT_REQUEST_SEED").unwrap_or_else(|_| client_id.clone());

        Self {
            endpoint: endpoint.to_string(),
            client_id,
            request_id_seed,
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

        Ok(ChatBridgeReply { text })
    }
}

#[derive(Debug)]
enum AgentError {
    Transport(tonic::transport::Error),
    Status(tonic::Status),
    Gpt(GptClientError),
}

impl From<tonic::transport::Error> for AgentError {
    fn from(err: tonic::transport::Error) -> Self {
        AgentError::Transport(err)
    }
}

impl From<tonic::Status> for AgentError {
    fn from(err: tonic::Status) -> Self {
        AgentError::Status(err)
    }
}

impl From<GptClientError> for AgentError {
    fn from(err: GptClientError) -> Self {
        AgentError::Gpt(err)
    }
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentError::Transport(err) => write!(f, "transport error: {err}"),
            AgentError::Status(err) => write!(f, "gRPC status: {err}"),
            AgentError::Gpt(err) => write!(f, "assistant error: {err}"),
        }
    }
}

impl std::error::Error for AgentError {}

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
}
