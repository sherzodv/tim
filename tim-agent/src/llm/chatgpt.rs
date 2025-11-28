use std::collections::HashMap;
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

fn silence_tool() -> ToolDefinition {
    ToolDefinition {
        kind: "function".to_string(),
        function: ToolFunction {
            name: "TIM-LLM-SILENCE".to_string(),
            description: "Use when you choose to not respond.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "reason": {
                        "type": "string",
                        "description": "The reason for choosing to remain silent."
                    },
                },
                "required": ["reason"],
                "additionalProperties": false
            }),
        },
    }
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
            tools: Some(vec![silence_tool()]),
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
            // Track tool call ids so later deltas without an id still map to the same call.
            let mut call_ids: HashMap<(usize, usize), String> = HashMap::new();
            while let Some(next) = events.next().await {
                let results = match next {
                    Ok(ev) => map_sse_event(ev, &mut call_ids),
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

fn map_sse_event(
    event: eventsource_stream::Event,
    call_ids: &mut HashMap<(usize, usize), String>,
) -> Vec<Result<LlmStreamEvent, LlmError>> {
    trace!("sse event: {}", event.data);
    if event.data.trim() == "[DONE]" {
        return vec![Ok(LlmStreamEvent::Completed)];
    }

    let chunk = match serde_json::from_str::<OaiChunk>(&event.data) {
        Ok(chunk) => chunk,
        Err(err) => return vec![Err(LlmError::Stream(err.to_string()))],
    };

    map_chunk(chunk, call_ids)
}

fn map_chunk(
    chunk: OaiChunk,
    call_ids: &mut HashMap<(usize, usize), String>,
) -> Vec<Result<LlmStreamEvent, LlmError>> {
    let mut out = Vec::new();
    for (choice_idx, choice) in chunk.choices.into_iter().enumerate() {
        if let Some(delta) = choice.delta {
            append_tool_calls(
                choice_idx,
                &choice.finish_reason,
                delta.tool_calls,
                call_ids,
                &mut out,
            );
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

fn append_tool_calls(
    choice_idx: usize,
    finish_reason: &Option<String>,
    tool_calls: Option<Vec<OaiToolCall>>,
    call_ids: &mut HashMap<(usize, usize), String>,
    out: &mut Vec<Result<LlmStreamEvent, LlmError>>,
) {
    let Some(tool_calls) = tool_calls else { return };

    for call in tool_calls {
        let id = resolve_call_id(choice_idx, &call, call_ids);
        let args = call
            .function
            .as_ref()
            .and_then(|f| f.arguments.clone())
            .unwrap_or_default();
        let name = call.function.as_ref().and_then(|f| f.name.clone());
        let finished = finish_reason.as_deref() == Some("tool_calls");
        out.push(Ok(LlmStreamEvent::ToolCallDelta {
            id,
            name,
            arguments_delta: args,
            finished,
        }));
    }
}

fn resolve_call_id(
    choice_idx: usize,
    call: &OaiToolCall,
    call_ids: &mut HashMap<(usize, usize), String>,
) -> String {
    let index = call.index.unwrap_or(0);
    let key = (choice_idx, index);
    let id = call
        .id
        .as_ref()
        .cloned()
        .or_else(|| call_ids.get(&key).cloned())
        .unwrap_or_else(|| format!("choice{choice_idx}-call{index}"));
    if let Some(explicit_id) = call.id.clone() {
        call_ids.insert(key, explicit_id);
    } else {
        call_ids.entry(key).or_insert_with(|| id.clone());
    }
    id
}
