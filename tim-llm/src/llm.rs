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

/// One entry of the conversation given to the model.
#[derive(Debug)]
pub enum LlmItem {
    User(String),
    Assistant(String),
    /// A turn exactly as the model produced it, see [`LlmTurn::items`]; passing it back
    /// verbatim keeps the model's reasoning and tool calls intact.
    Turn(Vec<serde_json::Value>),
    /// The result of a tool call the model made in an earlier turn.
    ToolOutput {
        call_id: String,
        output: String,
    },
}

#[derive(Debug)]
pub struct LlmReq<'a> {
    pub sysp: &'a str,
    pub inputs: &'a [LlmItem],
    pub tools: &'a [LlmTool],
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
    /// Reasoning effort for models that think before answering; none when absent.
    pub reasoning: Option<String>,
}

impl Debug for LlmConf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LlmConf")
            .field("endpoint", &self.endpoint)
            .field("model", &self.model)
            .field("temperature", &self.temperature)
            .field("reasoning", &self.reasoning)
            .finish()
    }
}

/// A function the model may call, described independently of any provider.
/// Names are limited to letters, digits, `_` and `-` by the providers.
#[derive(Debug)]
pub struct LlmTool {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// Lets the model stay silent and say why.
pub fn silence_tool() -> LlmTool {
    LlmTool {
        name: SILENCE_TOOL_NAME.to_string(),
        description: "Use when you choose to not respond.".to_string(),
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
    /// A finished output item, kept as the provider sent it.
    Item(serde_json::Value),
    Completed,
}

pub type LlmStream = Pin<Box<dyn Stream<Item = Result<LlmStreamEvent, LlmError>> + Send>>;

#[derive(Debug)]
pub struct LlmToolCall {
    pub call_id: String,
    pub name: String,
    pub arguments: String,
}

/// Everything one model turn produced.
#[derive(Debug, Default)]
pub struct LlmTurn {
    pub text: String,
    pub tool_calls: Vec<LlmToolCall>,
    /// All output items in order, to pass back as [`LlmItem::Turn`].
    pub items: Vec<serde_json::Value>,
}

#[async_trait]
pub trait Llm: Send + Sync {
    async fn chat_stream(&self, req: &LlmReq<'_>) -> Result<LlmStream, LlmError>;

    /// Runs one turn to completion.
    async fn turn(&self, req: &LlmReq<'_>) -> Result<LlmTurn, LlmError> {
        let mut stream = self.chat_stream(req).await?;
        collect(&mut stream).await
    }

    /// Runs one turn and reads it as a reply or a silence.
    async fn chat(&self, req: &LlmReq<'_>) -> Result<LlmRes, LlmError> {
        let turn = self.turn(req).await?;

        if let Some(reason) = silence_reason(&turn.tool_calls) {
            return Ok(LlmRes::NoResponse(reason));
        }

        match turn.text.trim() {
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

async fn collect(stream: &mut LlmStream) -> Result<LlmTurn, LlmError> {
    let mut turn = LlmTurn::default();

    while let Some(item) = stream.next().await {
        match item? {
            LlmStreamEvent::ContentDelta(delta) => {
                trace!("LLM content delta: {:?}", delta);
                turn.text.push_str(&delta);
            }
            LlmStreamEvent::Item(item) => {
                if let Some(call) = tool_call(&item) {
                    trace!(
                        "LLM tool call - name: {}, args: {:?}",
                        call.name,
                        call.arguments
                    );
                    turn.tool_calls.push(call);
                }
                turn.items.push(item);
            }
            LlmStreamEvent::Completed => {
                debug!("LLM stream completed");
                break;
            }
        }
    }

    Ok(turn)
}

fn tool_call(item: &serde_json::Value) -> Option<LlmToolCall> {
    if item["type"].as_str() != Some("function_call") {
        return None;
    }
    Some(LlmToolCall {
        call_id: item["call_id"].as_str()?.to_string(),
        name: item["name"].as_str()?.to_string(),
        arguments: item["arguments"].as_str().unwrap_or_default().to_string(),
    })
}

fn silence_reason(tool_calls: &[LlmToolCall]) -> Option<String> {
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
