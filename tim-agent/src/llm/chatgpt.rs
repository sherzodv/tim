use std::fmt;
use std::fmt::Debug;

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

pub const OPENAI_DEFAULT_ENDPOINT: &str = "https://api.openai.com/v1/responses";
pub const OPENAI_DEFAULT_MODEL: &str = "gpt-4.1";

const STR_DBG_LEN: usize = 40;
const VEC_DBG_LEN: usize = 3;

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
struct ResponsesReq {
    model: String,
    input: Vec<InputMessage>,
    temperature: f32,
    stream: bool,
    store: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ToolDefinition>>,
}

impl Debug for ResponsesReq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let last3: Vec<&InputMessage> = self.input.iter().rev().take(VEC_DBG_LEN).collect();
        f.debug_struct("ResponsesReq")
            .field("m", &self.model)
            .field("t", &self.temperature)
            .field("input", &format!("{:?}", last3))
            .finish()
    }
}

#[derive(Serialize)]
struct InputMessage {
    role: &'static str,
    content: Vec<InputContent>,
}

impl Debug for InputMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let content_preview = if let Some(first_content) = self.content.first() {
            match first_content {
                InputContent::InputText { text } => {
                    text.chars().take(STR_DBG_LEN).collect::<String>()
                }
                InputContent::OutputText { text } => {
                    text.chars().take(STR_DBG_LEN).collect::<String>()
                }
            }
        } else {
            "no-content".to_string()
        };
        f.debug_struct("InputMessage")
            .field("role", &self.role)
            .field("content_preview", &content_preview)
            .finish()
    }
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum InputContent {
    InputText { text: String },
    OutputText { text: String },
}

#[derive(Serialize)]
struct ToolDefinition {
    #[serde(rename = "type")]
    kind: String,
    name: String,
    description: String,
    parameters: serde_json::Value,
}

fn silence_tool() -> ToolDefinition {
    ToolDefinition {
        kind: "function".to_string(),
        name: "TIM-LLM-SILENCE".to_string(),
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

#[derive(Deserialize)]
struct SseEvent {
    #[serde(rename = "type")]
    kind: String,
    delta: Option<String>,
    item: Option<serde_json::Value>,
    _response: Option<serde_json::Value>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ResponseItem {
    FunctionCall {
        id: Option<String>,
        call_id: Option<String>,
        name: String,
        arguments: String,
    },
    #[serde(other)]
    Other,
}

#[async_trait]
impl Llm for ChatGpt {
    async fn chat_stream(&self, req: &LlmReq<'_>) -> Result<ResponseStream, LlmError> {
        if req.inputs.is_empty() && req.sysp.trim().is_empty() {
            return Err(LlmError::EmptyPrompt);
        }

        let mut input = Vec::with_capacity(req.inputs.len() + 1);
        input.push(InputMessage {
            role: "system",
            content: vec![InputContent::InputText {
                text: req.sysp.trim().to_string(),
            }],
        });
        for ev in req.inputs {
            let content = match ev.role {
                "assistant" => InputContent::OutputText {
                    text: ev.content.clone(),
                },
                _ => InputContent::InputText {
                    text: ev.content.clone(),
                },
            };
            input.push(InputMessage {
                role: ev.role,
                content: vec![content],
            });
        }

        let payload = ResponsesReq {
            model: self.model.clone(),
            input,
            temperature: self.temperature,
            stream: true,
            store: false,
            tools: Some(vec![silence_tool()]),
        };

        debug!("chatgpt req: {:?}", payload);

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
    let parsed = match serde_json::from_str::<SseEvent>(&event.data) {
        Ok(val) => val,
        Err(err) => return vec![Err(LlmError::Stream(err.to_string()))],
    };

    match parsed.kind.as_str() {
        "response.output_text.delta" => parsed
            .delta
            .map(LlmStreamEvent::ContentDelta)
            .map(|ev| vec![Ok(ev)])
            .unwrap_or_default(),
        "response.output_item.done" => {
            if let Some(item_val) = parsed.item {
                match serde_json::from_value::<ResponseItem>(item_val) {
                    Ok(ResponseItem::FunctionCall {
                        id,
                        call_id,
                        name,
                        arguments,
                    }) => {
                        let id = call_id.or(id).unwrap_or_else(|| name.clone());
                        vec![Ok(LlmStreamEvent::ToolCallDelta {
                            id,
                            name: Some(name),
                            arguments_delta: arguments,
                            finished: true,
                        })]
                    }
                    _ => Vec::new(),
                }
            } else {
                Vec::new()
            }
        }
        "response.completed" => vec![Ok(LlmStreamEvent::Completed)],
        _ => Vec::new(),
    }
}
