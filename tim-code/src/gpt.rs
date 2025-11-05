pub mod chatgpt;

use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::time::Duration;

use async_trait::async_trait;

/// Role of a message in a GPT-style chat exchange.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GptMessageRole {
    System,
    User,
    Assistant,
    Tool,
}

impl GptMessageRole {
    /// Converts the role into the canonical string representation used by APIs.
    pub fn as_str(&self) -> &'static str {
        match self {
            GptMessageRole::System => "system",
            GptMessageRole::User => "user",
            GptMessageRole::Assistant => "assistant",
            GptMessageRole::Tool => "tool",
        }
    }
}

/// A single message that will be sent to or received from a GPT provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GptMessage {
    pub role: GptMessageRole,
    pub content: String,
}

/// Shared controls for text generation style APIs.
#[derive(Debug, Clone, PartialEq)]
pub struct GptGenerationControls {
    pub max_output_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub timeout: Option<Duration>,
}

impl Default for GptGenerationControls {
    fn default() -> Self {
        Self {
            max_output_tokens: None,
            temperature: None,
            top_p: None,
            timeout: None,
        }
    }
}

impl GptGenerationControls {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Token accounting returned by GPT providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GptUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// Request payload for chat-completion style APIs.
#[derive(Debug, Clone)]
pub struct GptChatRequest {
    pub model: String,
    pub messages: Vec<GptMessage>,
    pub controls: GptGenerationControls,
}

impl GptChatRequest {
    pub fn new(model: impl Into<String>, messages: Vec<GptMessage>) -> Self {
        Self {
            model: model.into(),
            messages,
            controls: GptGenerationControls::default(),
        }
    }

    pub fn with_controls(mut self, controls: GptGenerationControls) -> Self {
        self.controls = controls;
        self
    }
}

/// A single chat choice returned by the provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GptChatChoice {
    pub message: GptMessage,
    pub finish_reason: Option<String>,
}

/// Response wrapper for chat-completion APIs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GptChatResponse {
    pub choices: Vec<GptChatChoice>,
    pub usage: Option<GptUsage>,
    pub provider_request_id: Option<String>,
}

impl GptChatResponse {
    pub fn single(message: GptMessage) -> Self {
        Self {
            choices: vec![GptChatChoice {
                message,
                finish_reason: None,
            }],
            usage: None,
            provider_request_id: None,
        }
    }
}

/// Request payload for classic text completion style APIs.
#[derive(Debug, Clone)]
pub struct GptCompletionRequest {
    pub model: String,
    pub prompt: String,
    pub suffix: Option<String>,
    pub controls: GptGenerationControls,
}

impl GptCompletionRequest {
    pub fn new(model: impl Into<String>, prompt: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            prompt: prompt.into(),
            suffix: None,
            controls: GptGenerationControls::default(),
        }
    }

    pub fn with_suffix(mut self, suffix: impl Into<String>) -> Self {
        self.suffix = Some(suffix.into());
        self
    }

    pub fn with_controls(mut self, controls: GptGenerationControls) -> Self {
        self.controls = controls;
        self
    }
}

/// A single completion choice returned by the provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GptCompletionChoice {
    pub text: String,
    pub finish_reason: Option<String>,
}

/// Response wrapper for completion APIs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GptCompletionResponse {
    pub choices: Vec<GptCompletionChoice>,
    pub usage: Option<GptUsage>,
    pub provider_request_id: Option<String>,
}

impl GptCompletionResponse {
    pub fn single(text: impl Into<String>) -> Self {
        Self {
            choices: vec![GptCompletionChoice {
                text: text.into(),
                finish_reason: None,
            }],
            usage: None,
            provider_request_id: None,
        }
    }
}

/// Failure modes for GPT client implementations.
#[derive(Debug)]
pub enum GptClientError {
    Transport(String),
    InvalidRequest(String),
    Provider(String),
}

impl Display for GptClientError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            GptClientError::Transport(msg) => write!(f, "transport error: {msg}"),
            GptClientError::InvalidRequest(msg) => write!(f, "invalid request: {msg}"),
            GptClientError::Provider(msg) => write!(f, "provider error: {msg}"),
        }
    }
}

impl Error for GptClientError {}

pub type GptClientResult<T> = Result<T, GptClientError>;

/// Abstraction over chat and completion capable GPT providers.
#[async_trait]
pub trait GptClient: Send + Sync {
    fn provider_name(&self) -> &'static str;

    async fn chat(&self, request: GptChatRequest) -> GptClientResult<GptChatResponse>;

    async fn completion(
        &self,
        request: GptCompletionRequest,
    ) -> GptClientResult<GptCompletionResponse>;
}

pub use chatgpt::ChatGptClient;
