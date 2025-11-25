use std::fmt;

use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures::StreamExt;
use reqwest::Client;
use serde::Deserialize;
use serde::Serialize;
use serde_json::json;
use tokio::sync::mpsc;
use tracing::debug;
use tracing::trace;

use super::llm::Llm;
use super::llm::LlmError;
use super::llm::LlmReq;
use super::llm::LlmStreamEvent;
use super::llm::ResponseStream;

pub const OPENAI_DEFAULT_ENDPOINT: &str = "https://api.openai.com/v1/chat/completions";
pub const OPENAI_DEFAULT_MODEL: &str = "gpt-4o-mini";

#[derive(Clone)]
pub struct ChatGpt {
    client: Client,
    api_key: String,
    endpoint: String,
    model: String,
    temperature: f32,
}

impl fmt::Debug for ChatGpt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChatGpt")
            .field("endpoint", &self.endpoint)
            .field("model", &self.model)
            .field("temperature", &self.temperature)
            .finish()
    }
}

impl ChatGpt {
    pub fn new(
        api_key: String,
        endpoint: String,
        model: String,
        temperature: f32,
    ) -> Result<Self, LlmError> {
        if api_key.trim().is_empty() {
            return Err(LlmError::MissingApiKey);
        }
        let endpoint = if endpoint.trim().is_empty() {
            OPENAI_DEFAULT_ENDPOINT.to_string()
        } else {
            endpoint
        };
        let model = if model.trim().is_empty() {
            OPENAI_DEFAULT_MODEL.to_string()
        } else {
            model
        };

        Ok(Self {
            client: Client::new(),
            api_key,
            endpoint,
            model,
            temperature: temperature.max(0.0),
        })
    }
}

#[derive(Serialize)]
struct StreamChatReq {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
    stream: bool,
    tools: Option<Vec<ToolDefinition>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<serde_json::Value>,
}

#[derive(Serialize)]
struct ChatMessage {
    role: &'static str,
    content: String,
}

#[derive(Serialize)]
struct ToolDefinition {
    #[serde(rename = "type")]
    kind: String,
    function: ToolFunction,
}

#[derive(Serialize)]
struct ToolFunction {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Deserialize)]
struct OaiChunk {
    choices: Vec<OaiChoice>,
}

#[derive(Deserialize)]
struct OaiChoice {
    delta: Option<OaiDelta>,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct OaiDelta {
    content: Option<String>,
    tool_calls: Option<Vec<OaiToolCall>>,
}

#[derive(Deserialize)]
struct OaiToolCall {
    index: Option<usize>,
    id: Option<String>,
    function: Option<OaiFunctionCall>,
}

#[derive(Deserialize)]
struct OaiFunctionCall {
    name: Option<String>,
    arguments: Option<String>,
}

#[async_trait]
impl Llm for ChatGpt {
    async fn chat_stream(&self, req: &LlmReq<'_>) -> Result<ResponseStream, LlmError> {
        if req.msg.trim().is_empty() {
            return Err(LlmError::EmptyPrompt);
        }

        let silence_tool = ToolDefinition {
            kind: "function".to_string(),
            function: ToolFunction {
                name: "TIM-LLM-SILENCE".to_string(),
                description: "Use when you choose to not respond.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                }),
            },
        };

        let payload = StreamChatReq {
            model: self.model.clone(),
            messages: vec![
                ChatMessage {
                    role: "system",
                    content: format!("{}\n{}", req.sysp.trim(), req.userp.trim()),
                },
                ChatMessage {
                    role: "user",
                    content: req.msg.trim().to_string(),
                },
            ],
            temperature: self.temperature,
            stream: true,
            tools: Some(vec![silence_tool]),
            tool_choice: None,
        };
        debug!(
            "chat_stream request endpoint={} model={} temperature={} prompt_len={}",
            self.endpoint,
            self.model,
            self.temperature,
            req.msg.len()
        );

        let response = self
            .client
            .post(&self.endpoint)
            .bearer_auth(&self.api_key)
            .json(&payload)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(LlmError::Api(format!("status {status}: {body}")));
        }

        let (tx, rx) = mpsc::channel(32);
        let mut events = response.bytes_stream().eventsource();
        tokio::spawn(async move {
            while let Some(next) = events.next().await {
                let results = match next {
                    Ok(ev) => map_sse_event(ev),
                    Err(err) => vec![Err(LlmError::Stream(err.to_string()))],
                };
                for item in results {
                    if tx.send(item).await.is_err() {
                        return;
                    }
                }
            }
        });

        Ok(ResponseStream { rx_event: rx })
    }
}

fn map_sse_event(event: eventsource_stream::Event) -> Vec<Result<LlmStreamEvent, LlmError>> {
    trace!("sse event: {}", event.data);
    if event.data.trim() == "[DONE]" {
        return vec![Ok(LlmStreamEvent::Completed)];
    }

    match serde_json::from_str::<OaiChunk>(&event.data) {
        Ok(chunk) => {
            let mut out = Vec::new();
            for (choice_idx, choice) in chunk.choices.into_iter().enumerate() {
                if let Some(delta) = choice.delta {
                    if let Some(tool_calls) = delta.tool_calls {
                        for call in tool_calls {
                            let id = call.id.unwrap_or_else(|| {
                                format!("choice{choice_idx}-call{}", call.index.unwrap_or(0))
                            });
                            let args = call
                                .function
                                .as_ref()
                                .and_then(|f| f.arguments.clone())
                                .unwrap_or_default();
                            let name = call.function.as_ref().and_then(|f| f.name.clone());
                            let finished = choice
                                .finish_reason
                                .as_deref()
                                .map(|r| r == "tool_calls")
                                .unwrap_or(false);
                            out.push(Ok(LlmStreamEvent::ToolCallDelta {
                                id,
                                name,
                                arguments_delta: args,
                                finished,
                            }));
                        }
                    }
                    if let Some(content) = delta.content {
                        out.push(Ok(LlmStreamEvent::ContentDelta(content)));
                    }
                }
                if choice.finish_reason.as_deref() == Some("stop") {
                    out.push(Ok(LlmStreamEvent::Completed));
                }
            }
            out
        }
        Err(err) => vec![Err(LlmError::Stream(err.to_string()))],
    }
}
