//! One timeline entry per persisted LLM attempt, updated as events arrive.

use std::collections::BTreeMap;

use chrono::Local;
use serde_json::{json, Value};

#[derive(Debug, Clone)]
pub struct LlmCall {
    pub timite_id: u64,
    pub task_id: u64,
    pub call_id: u64,
    pub timestamp: u64,
    pub interrupted: bool,
    started: Value,
    response: Value,
    finished: Value,
    text: String,
    output: BTreeMap<u64, Value>,
    events: Vec<(u64, u64, String)>,
}

impl LlmCall {
    pub fn new(timite_id: u64, task_id: u64, call_id: u64, timestamp: u64) -> Self {
        Self {
            timite_id,
            task_id,
            call_id,
            timestamp,
            interrupted: false,
            started: Value::Null,
            response: Value::Null,
            finished: Value::Null,
            text: String::new(),
            output: BTreeMap::new(),
            events: Vec::new(),
        }
    }

    pub fn record(&mut self, event_id: u64, timestamp: u64, payload: String) {
        self.interrupted = false;
        if let Ok(event) = serde_json::from_str::<Value>(&payload) {
            match event["type"].as_str() {
                Some("llm.call.started") => self.started = event,
                Some("llm.call.response") => self.response = event,
                Some("llm.call.finished") => self.finished = event,
                Some("response.output_text.delta") => {
                    self.text.push_str(event["delta"].as_str().unwrap_or(""));
                }
                Some("response.output_item.done") => {
                    if let Some(item) = event.get("item") {
                        let index = event["output_index"]
                            .as_u64()
                            .unwrap_or(self.output.len() as u64);
                        self.output.insert(index, item.clone());
                    }
                }
                Some("response.completed" | "response.failed" | "response.incomplete") => {
                    if let Some(output) =
                        event.pointer("/response/output").and_then(Value::as_array)
                    {
                        self.output = output
                            .iter()
                            .enumerate()
                            .map(|(i, item)| (i as u64, item.clone()))
                            .collect();
                    }
                }
                _ => {}
            }
        }
        self.events.push((event_id, timestamp, payload));
    }

    pub fn status(&self) -> &str {
        self.finished["status"].as_str().unwrap_or_else(|| {
            if self.interrupted || self.started.is_null() {
                "unknown"
            } else {
                "running"
            }
        })
    }

    pub fn label(&self) -> String {
        let model = self.started["model"].as_str().unwrap_or("LLM");
        let duration = self.finished["duration_ms"].as_u64().or_else(|| {
            (self.status() == "running").then(|| {
                (Local::now().timestamp_millis().max(0) as u64).saturating_sub(self.timestamp)
            })
        });
        let duration = duration
            .map(|ms| format!(" · {}.{:03}s", ms / 1000, ms % 1000))
            .unwrap_or_default();
        let http = self.response["http_status"]
            .as_u64()
            .map(|status| format!(" · HTTP {status}"))
            .unwrap_or_default();
        format!(
            "{model} · #{}/{} · {}{http}{duration}",
            self.task_id,
            self.call_id,
            self.status()
        )
    }

    pub fn details(&self, tab: usize) -> String {
        match tab {
            1 => self
                .started
                .get("request")
                .map(pretty)
                .unwrap_or_else(|| "Request not recorded".to_string()),
            2 => self.response_text(),
            3 => self
                .events
                .iter()
                .map(|(id, timestamp, payload)| {
                    let body = serde_json::from_str::<Value>(payload)
                        .map(|value| pretty(&value))
                        .unwrap_or_else(|_| payload.clone());
                    format!("Event #{id} · timestamp {timestamp} ms\n{body}")
                })
                .collect::<Vec<_>>()
                .join("\n\n"),
            _ => pretty(&json!({
                "task_id": self.task_id,
                "llm_call_id": self.call_id,
                "provider": self.started["provider"],
                "model": self.started["model"],
                "endpoint": self.started["endpoint"],
                "status": self.status(),
                "duration_ms": self.finished["duration_ms"],
                "http_status": self.response["http_status"],
                "request_id": self.response["request_id"],
                "error": self.finished["error"],
                "usage": self.finished["usage"],
            })),
        }
    }

    fn response_text(&self) -> String {
        let mut parts = Vec::new();
        if !self.text.is_empty() {
            parts.push(self.text.clone());
        }
        for item in self.output.values() {
            match item["type"].as_str() {
                Some("message") if self.text.is_empty() => {
                    if let Some(content) = item["content"].as_array() {
                        parts.extend(
                            content
                                .iter()
                                .filter_map(|part| part["text"].as_str().map(str::to_string)),
                        );
                    }
                }
                Some("function_call" | "custom_tool_call") => parts.push(pretty(item)),
                _ => {}
            }
        }
        if parts.is_empty() {
            "No response text or tool calls recorded yet".to_string()
        } else {
            parts.join("\n\n")
        }
    }
}

fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).expect("JSON value serializes")
}
