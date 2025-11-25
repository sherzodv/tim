use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use async_trait::async_trait;
use futures::Stream;
use futures::StreamExt;
use thiserror::Error;
use tokio::sync::mpsc;

#[derive(Debug)]
pub struct LlmReq<'a> {
    pub sysp: &'a str,
    pub userp: &'a str,
    pub msg: &'a str,
}

pub enum LlmRes {
    Reply(String),
    NoResponse,
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

impl Stream for ResponseStream {
    type Item = Result<LlmStreamEvent, LlmError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.rx_event.poll_recv(cx)
    }
}

#[async_trait]
pub trait Llm: Send + Sync {
    async fn chat_stream(&self, req: &LlmReq<'_>) -> Result<ResponseStream, LlmError>;

    async fn chat(&self, req: &LlmReq<'_>) -> Result<LlmRes, LlmError> {
        let mut stream = self.chat_stream(req).await?;
        let mut message = String::new();
        let mut no_response = false;
        while let Some(item) = stream.next().await {
            match item? {
                LlmStreamEvent::ContentDelta(delta) => message.push_str(&delta),
                LlmStreamEvent::ToolCallDelta {
                    id,
                    name,
                    arguments_delta,
                    finished,
                } => {
                    if name == Some("TIM-LLM-SILENCE".to_string()) {
                        no_response = true;
                        break;
                    }
                    let _ = (id, name, arguments_delta, finished);
                }
                LlmStreamEvent::Completed => break,
            }
        }
        if no_response {
            return Ok(LlmRes::NoResponse);
        }
        if message.trim().is_empty() {
            return Err(LlmError::MissingContent);
        }
        Ok(LlmRes::Reply(message.trim().to_string()))
    }
}
