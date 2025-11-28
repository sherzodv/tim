use std::collections::HashMap;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use async_trait::async_trait;
use futures::Stream;
use futures::StreamExt;
use thiserror::Error;
use tokio::sync::mpsc;
use tracing::debug;
use tracing::trace;
use tracing::warn;

#[derive(Debug)]
pub struct LlmReq<'a> {
    pub sysp: &'a str,
    pub msg: &'a str,
}

pub enum LlmRes {
    Reply(String),
    NoResponse(String), // Contains the reason for silence
}

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("llm prompt is empty")]
    EmptyPrompt,
    #[error("missing OpenAI API key (set OPENAI_API_KEY)")]
    MissingApiKey,
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
    ToolCallDelta {
        id: String,
        name: Option<String>,
        arguments_delta: String,
        finished: bool,
    },
    Completed,
}

pub struct ResponseStream {
    pub(crate) rx_event: mpsc::Receiver<Result<LlmStreamEvent, LlmError>>,
}

#[derive(Debug)]
struct ToolCall {
    _id: String,
    name: Option<String>,
    arguments: String,
}

impl ToolCall {
    fn new(id: String) -> Self {
        Self {
            _id: id,
            name: None,
            arguments: String::new(),
        }
    }
}

impl Stream for ResponseStream {
    type Item = Result<LlmStreamEvent, LlmError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.rx_event.poll_recv(cx)
    }
}

#[derive(Debug)]
struct CollectedStream {
    message: String,
    tool_calls: Vec<ToolCall>,
}

#[async_trait]
pub trait Llm: Send + Sync {
    async fn chat_stream(&self, req: &LlmReq<'_>) -> Result<ResponseStream, LlmError>;

    async fn chat(&self, req: &LlmReq<'_>) -> Result<LlmRes, LlmError> {
        let mut stream = self.chat_stream(req).await?;
        let collected = collect_message_and_tool_calls(&mut stream).await?;

        if let Some(reason) = silence_reason(&collected.tool_calls) {
            return Ok(LlmRes::NoResponse(reason));
        }

        match collected.message.trim() {
            "" => Err(LlmError::MissingContent),
            content => Ok(LlmRes::Reply(content.to_string())),
        }
    }
}

async fn collect_message_and_tool_calls(
    stream: &mut ResponseStream,
) -> Result<CollectedStream, LlmError> {
    let mut message = String::new();
    let mut tool_calls: HashMap<String, ToolCall> = HashMap::new();

    while let Some(item) = stream.next().await {
        match item? {
            LlmStreamEvent::ContentDelta(delta) => {
                trace!("LLM content delta: {:?}", delta);
                message.push_str(&delta);
            }
            LlmStreamEvent::ToolCallDelta {
                id,
                name,
                arguments_delta,
                finished,
            } => {
                trace!(
                    "LLM tool call delta - id: {}, name: {:?}, args_delta: {:?}, finished: {}",
                    id,
                    name,
                    arguments_delta,
                    finished
                );
                let entry = tool_calls
                    .entry(id.clone())
                    .or_insert_with(|| ToolCall::new(id.clone()));
                if let Some(name) = name {
                    entry.name = Some(name);
                }
                entry.arguments.push_str(&arguments_delta);
            }
            LlmStreamEvent::Completed => {
                debug!("LLM stream completed");
                break;
            }
        }
    }

    Ok(CollectedStream {
        message,
        tool_calls: tool_calls.into_values().collect(),
    })
}

fn silence_reason(tool_calls: &[ToolCall]) -> Option<String> {
    let call = tool_calls
        .iter()
        .find(|call| call.name.as_deref() == Some("TIM-LLM-SILENCE"))?;

    Some(parse_silence_reason(&call.arguments))
}

fn parse_silence_reason(args: &str) -> String {
    if args.trim().is_empty() {
        debug!("TIM-LLM-SILENCE called without arguments, defaulting reason");
        return "No reason provided".to_string();
    }

    match serde_json::from_str::<serde_json::Value>(args) {
        Ok(parsed) => parsed["reason"]
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                warn!("TIM-LLM-SILENCE missing 'reason' field. Args: {}", args);
                "No reason provided".to_string()
            }),
        Err(e) => {
            warn!(
                "Failed to parse TIM-LLM-SILENCE arguments: {}. Args: {}",
                e, args
            );
            "No reason provided".to_string()
        }
    }
}
