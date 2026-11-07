//! Wire format of the OpenAI Responses API: request types, transport and SSE parsing.
//! Providers speaking this format own their defaults and any fields beyond the base request.

use std::fmt;
use std::fmt::Debug;

use eventsource_stream::Eventsource;
use futures::stream;
use futures::StreamExt;
use futures::TryStreamExt;
use reqwest::Client;
use serde::Deserialize;
use serde::Serialize;
use tracing::debug;
use tracing::trace;

use crate::llm::LlmConf;
use crate::llm::LlmError;
use crate::llm::LlmItem;
use crate::llm::LlmReq;
use crate::llm::LlmStream;
use crate::llm::LlmStreamEvent;
use crate::llm::LlmTool;

const STR_DBG_LEN: usize = 40;
const VEC_DBG_LEN: usize = 3;

#[derive(Clone)]
pub(crate) struct ResponsesClient {
    http: Client,
    api_key: String,
    endpoint: String,
    model: String,
    temperature: f32,
}

impl Debug for ResponsesClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResponsesClient")
            .field("endpoint", &self.endpoint)
            .field("model", &self.model)
            .field("temperature", &self.temperature)
            .finish()
    }
}

impl ResponsesClient {
    pub(crate) fn new(
        conf: &LlmConf,
        default_endpoint: &str,
        default_model: &str,
    ) -> Result<Self, LlmError> {
        if conf.api_key.trim().is_empty() {
            return Err(LlmError::MissingApiKey);
        }
        let endpoint = conf
            .endpoint
            .clone()
            .unwrap_or_else(|| default_endpoint.to_string());
        let model = if conf.model.trim().is_empty() {
            default_model.to_string()
        } else {
            conf.model.clone()
        };

        Ok(Self {
            http: Client::new(),
            api_key: conf.api_key.clone(),
            endpoint,
            model,
            temperature: conf.temperature.max(0.0),
        })
    }

    /// The base request every provider sends; providers extend it with their own fields.
    pub(crate) fn request(&self, req: &LlmReq<'_>) -> Result<ResponsesReq, LlmError> {
        if req.inputs.is_empty() && req.sysp.trim().is_empty() {
            return Err(LlmError::EmptyPrompt);
        }

        let mut input = Vec::with_capacity(req.inputs.len() + 1);
        input.push(InputItem::Message(system_message(req.sysp.trim())));
        input.extend(req.inputs.iter().flat_map(input_items));

        Ok(ResponsesReq {
            model: self.model.clone(),
            input,
            temperature: self.temperature,
            stream: true,
            store: false,
            tools: req.tools.iter().map(tool_definition).collect(),
        })
    }

    pub(crate) async fn stream<P: Serialize + Debug>(
        &self,
        payload: &P,
    ) -> Result<LlmStream, LlmError> {
        debug!("responses req: {:?}", payload);

        let payload = serde_json::to_value(payload)?;
        let request = self
            .http
            .post(&self.endpoint)
            .bearer_auth(&self.api_key)
            .json(&payload)
            .build()?;
        let started = LlmStreamEvent::Request {
            endpoint: self.endpoint.clone(),
            payload,
        };
        let http = self.http.clone();
        // Send only after the caller has observed the request event.
        let response = stream::once(async move {
            let response = http.execute(request).await?;
            Ok::<LlmStream, LlmError>(response_stream(response))
        })
        .try_flatten();
        Ok(Box::pin(stream::iter([Ok(started)]).chain(response)))
    }
}

fn response_stream(response: reqwest::Response) -> LlmStream {
    let status = response.status();
    let request_id = response
        .headers()
        .get("x-request-id")
        .or_else(|| response.headers().get("request-id"))
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let received = LlmStreamEvent::Response {
        http_status: status.as_u16(),
        request_id,
    };
    let events: LlmStream = if status.is_success() {
        Box::pin(response.bytes_stream().eventsource().flat_map(|next| {
            let mapped = match next {
                Ok(event) => map_sse_event(event),
                Err(err) => vec![Err(LlmError::Stream(err.to_string()))],
            };
            stream::iter(mapped)
        }))
    } else {
        Box::pin(stream::once(async move {
            let body = response.text().await?;
            Err(LlmError::Api(format!("status {status}: {body}")))
        }))
    };
    Box::pin(stream::iter([Ok(received)]).chain(events))
}

#[derive(Serialize)]
pub(crate) struct ResponsesReq {
    model: String,
    input: Vec<InputItem>,
    temperature: f32,
    stream: bool,
    store: bool,
    tools: Vec<ToolDefinition>,
}

impl Debug for ResponsesReq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let last3: Vec<&InputItem> = self.input.iter().rev().take(VEC_DBG_LEN).collect();
        f.debug_struct("ResponsesReq")
            .field("m", &self.model)
            .field("t", &self.temperature)
            .field("input", &format!("{:?}", last3))
            .finish()
    }
}

/// One entry of `input`; model turns go back as the raw items they came as.
#[derive(Serialize, Debug)]
#[serde(untagged)]
enum InputItem {
    Message(InputMessage),
    Raw(serde_json::Value),
    FunctionCallOutput(FunctionCallOutput),
}

#[derive(Serialize)]
struct InputMessage {
    role: &'static str,
    content: Vec<InputContent>,
}

#[derive(Serialize, Debug)]
struct FunctionCallOutput {
    #[serde(rename = "type")]
    kind: &'static str,
    call_id: String,
    output: String,
}

impl Debug for InputMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let content_preview = match self.content.first() {
            Some(InputContent::InputText { text }) | Some(InputContent::OutputText { text }) => {
                text.chars().take(STR_DBG_LEN).collect::<String>()
            }
            None => "no-content".to_string(),
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
    kind: &'static str,
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Deserialize)]
struct SseEvent {
    #[serde(rename = "type")]
    kind: String,
    delta: Option<String>,
    item: Option<serde_json::Value>,
}

fn system_message(text: &str) -> InputMessage {
    InputMessage {
        role: "system",
        content: vec![InputContent::InputText {
            text: text.to_string(),
        }],
    }
}

fn input_items(item: &LlmItem) -> Vec<InputItem> {
    match item {
        LlmItem::User(text) => vec![InputItem::Message(InputMessage {
            role: "user",
            content: vec![InputContent::InputText { text: text.clone() }],
        })],
        LlmItem::Assistant(text) => vec![InputItem::Message(InputMessage {
            role: "assistant",
            content: vec![InputContent::OutputText { text: text.clone() }],
        })],
        LlmItem::Turn(items) => items.iter().cloned().map(InputItem::Raw).collect(),
        LlmItem::ToolOutput { call_id, output } => {
            vec![InputItem::FunctionCallOutput(FunctionCallOutput {
                kind: "function_call_output",
                call_id: call_id.clone(),
                output: output.clone(),
            })]
        }
    }
}

fn tool_definition(tool: &LlmTool) -> ToolDefinition {
    ToolDefinition {
        kind: "function",
        name: tool.name.clone(),
        description: tool.description.clone(),
        parameters: tool.parameters.clone(),
    }
}

fn map_sse_event(event: eventsource_stream::Event) -> Vec<Result<LlmStreamEvent, LlmError>> {
    trace!("sse event: {}", event.data);
    let parsed = serde_json::from_str::<SseEvent>(&event.data);
    let mut mapped = vec![Ok(LlmStreamEvent::Raw(event.data))];
    let parsed = match parsed {
        Ok(val) => val,
        Err(err) => {
            mapped.push(Err(LlmError::Stream(err.to_string())));
            return mapped;
        }
    };

    mapped.extend(match parsed.kind.as_str() {
        "response.output_text.delta" => parsed
            .delta
            .map(LlmStreamEvent::ContentDelta)
            .map(|ev| vec![Ok(ev)])
            .unwrap_or_default(),
        "response.output_item.done" => parsed
            .item
            .map(LlmStreamEvent::Item)
            .map(|ev| vec![Ok(ev)])
            .unwrap_or_default(),
        "response.completed" => vec![Ok(LlmStreamEvent::Completed)],
        "response.failed" | "response.incomplete" | "error" => {
            vec![Err(LlmError::Api(parsed.kind))]
        }
        _ => Vec::new(),
    });
    mapped
}
