use std::fmt;
use std::fmt::Debug;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use futures::Stream;
use futures::StreamExt;
use serde_json::json;
use thiserror::Error;
use tracing::debug;
use tracing::trace;
use tracing::warn;

use crate::deepseek::DeepSeek;
use crate::openai::OpenAi;

pub const SILENCE_TOOL_NAME: &str = "TIM-LLM-SILENCE";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmRole {
    User,
    Assistant,
}

#[derive(Debug)]
pub struct LlmInputItem {
    pub role: LlmRole,
    pub content: String,
}

#[derive(Debug)]
pub struct LlmReq<'a> {
    pub sysp: &'a str,
    pub inputs: &'a [LlmInputItem],
}

pub enum LlmRes {
    Reply(String),
    NoResponse(String), // Contains the reason for silence
}

/// Connection settings every provider needs; `endpoint` overrides the provider default.
#[derive(Clone)]
pub struct LlmConf {
    pub api_key: String,
    pub endpoint: Option<String>,
    pub model: String,
    pub temperature: f32,
}

impl Debug for LlmConf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LlmConf")
            .field("endpoint", &self.endpoint)
            .field("model", &self.model)
            .field("temperature", &self.temperature)
            .finish()
    }
}

/// A function the model may call, described independently of any provider.
pub struct LlmTool {
    pub name: &'static str,
    pub description: &'static str,
    pub parameters: serde_json::Value,
}

/// The one tool every provider offers: lets the model stay silent and say why.
pub fn silence_tool() -> LlmTool {
    LlmTool {
        name: SILENCE_TOOL_NAME,
        description: "Use when you choose to not respond.",
        parameters: json!({
            "type": "object",
            "properties": {
                "reason": {
                    "type": "string",
                    "description": "Very short reason for choosing to remain silent."
                },
            },
            "required": ["reason"],
            "additionalProperties": false
        }),
    }
}

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("llm prompt is empty")]
    EmptyPrompt,
    #[error("llm api key is empty")]
    MissingApiKey,
    #[error("unknown llm provider: {0}")]
    UnknownProvider(String),
    #[error("http error while contacting LLM: {0}")]
    Http(#[from] reqwest::Error),
    #[error("failed to decode LLM response: {0}")]
    Response(#[from] serde_json::Error),
    #[error("LLM reported an error: {0}")]
    Api(String),
    #[error("LLM response missing message content")]
    MissingContent,
    #[error("llm stream error: {0}")]
    Stream(String),
}

#[derive(Debug)]
pub enum LlmStreamEvent {
    ContentDelta(String),
    ToolCall { name: String, arguments: String },
    Completed,
}

pub type LlmStream = Pin<Box<dyn Stream<Item = Result<LlmStreamEvent, LlmError>> + Send>>;

#[async_trait]
pub trait Llm: Send + Sync {
    async fn chat_stream(&self, req: &LlmReq<'_>) -> Result<LlmStream, LlmError>;

    async fn chat(&self, req: &LlmReq<'_>) -> Result<LlmRes, LlmError> {
        let mut stream = self.chat_stream(req).await?;
        let collected = collect(&mut stream).await?;

        if let Some(reason) = silence_reason(&collected.tool_calls) {
            return Ok(LlmRes::NoResponse(reason));
        }

        match collected.message.trim() {
            "" => Err(LlmError::MissingContent),
            content => Ok(LlmRes::Reply(content.to_string())),
        }
    }
}

/// Picks the implementation by provider name, e.g. `openai`.
pub fn build_llm(provider: &str, conf: &LlmConf) -> Result<Arc<dyn Llm>, LlmError> {
    match provider {
        "openai" => Ok(Arc::new(OpenAi::new(conf)?)),
        "deepseek" => Ok(Arc::new(DeepSeek::new(conf)?)),
        other => Err(LlmError::UnknownProvider(other.to_string())),
    }
}

#[derive(Debug)]
struct ToolCall {
    name: String,
    arguments: String,
}

#[derive(Debug, Default)]
struct Collected {
    message: String,
    tool_calls: Vec<ToolCall>,
}

async fn collect(stream: &mut LlmStream) -> Result<Collected, LlmError> {
    let mut collected = Collected::default();

    while let Some(item) = stream.next().await {
        match item? {
            LlmStreamEvent::ContentDelta(delta) => {
                trace!("LLM content delta: {:?}", delta);
                collected.message.push_str(&delta);
            }
            LlmStreamEvent::ToolCall { name, arguments } => {
                trace!("LLM tool call - name: {}, args: {:?}", name, arguments);
                collected.tool_calls.push(ToolCall { name, arguments });
            }
            LlmStreamEvent::Completed => {
                debug!("LLM stream completed");
                break;
            }
        }
    }

    Ok(collected)
}

fn silence_reason(tool_calls: &[ToolCall]) -> Option<String> {
    let call = tool_calls
        .iter()
        .find(|call| call.name == SILENCE_TOOL_NAME)?;

    Some(parse_silence_reason(&call.arguments))
}

fn parse_silence_reason(args: &str) -> String {
    if args.trim().is_empty() {
        debug!("{SILENCE_TOOL_NAME} called without arguments, defaulting reason");
        return "No reason provided".to_string();
    }

    match serde_json::from_str::<serde_json::Value>(args) {
        Ok(parsed) => parsed["reason"]
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                warn!("{SILENCE_TOOL_NAME} missing 'reason' field. Args: {}", args);
                "No reason provided".to_string()
            }),
        Err(e) => {
            warn!(
                "Failed to parse {SILENCE_TOOL_NAME} arguments: {}. Args: {}",
                e, args
            );
            "No reason provided".to_string()
        }
    }
}
