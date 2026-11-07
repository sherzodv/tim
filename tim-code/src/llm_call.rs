//! Persists one HTTP attempt alongside the model events used for history replay.

use std::time::Instant;

use serde_json::json;
use serde_json::Value;
use tim_llm::llm::LlmError;
use tim_llm::llm::LlmStreamEvent;

use crate::progress::Reporter;

pub struct LlmCall<'a> {
    reporter: &'a Reporter,
    call_id: u64,
    provider: &'a str,
    started: Instant,
    http_status: Option<u16>,
    request_id: Option<String>,
    usage: Option<Value>,
    finished: bool,
}

impl<'a> LlmCall<'a> {
    pub fn new(reporter: &'a Reporter, call_id: u64, provider: &'a str) -> Self {
        Self {
            reporter,
            call_id,
            provider,
            started: Instant::now(),
            http_status: None,
            request_id: None,
            usage: None,
            finished: false,
        }
    }

    pub fn record(&mut self, event: &LlmStreamEvent) {
        match event {
            LlmStreamEvent::Request { endpoint, payload } => self.report(json!({
                "type": "llm.call.started",
                "provider": self.provider,
                "model": payload.get("model"),
                "endpoint": endpoint,
                "request": payload,
            })),
            LlmStreamEvent::Response {
                http_status,
                request_id,
            } => {
                self.http_status = Some(*http_status);
                self.request_id = request_id.clone();
                self.report(json!({
                    "type": "llm.call.response",
                    "http_status": http_status,
                    "request_id": request_id,
                }));
            }
            LlmStreamEvent::Raw(payload) => {
                // Keep the provider's usage object verbatim, without estimating or summing.
                if let Ok(event) = serde_json::from_str::<Value>(payload) {
                    if let Some(usage) = event.pointer("/response/usage").filter(|v| !v.is_null()) {
                        self.usage = Some(usage.clone());
                    }
                }
                self.reporter.llm_event(self.call_id, payload.clone());
            }
            _ => {}
        }
    }

    pub fn finish(&mut self, error: Option<&LlmError>) {
        self.report_finished(
            if error.is_some() {
                "failed"
            } else {
                "succeeded"
            },
            error.map(ToString::to_string),
        );
        self.finished = true;
    }

    fn report_finished(&self, status: &str, error: Option<String>) {
        self.report(json!({
            "type": "llm.call.finished",
            "status": status,
            "duration_ms": self.started.elapsed().as_millis() as u64,
            "http_status": self.http_status,
            "request_id": self.request_id,
            "usage": self.usage,
            "error": error,
        }));
    }

    fn report(&self, payload: Value) {
        self.reporter.llm_event(self.call_id, payload.to_string());
    }
}

impl Drop for LlmCall<'_> {
    fn drop(&mut self) {
        if !self.finished {
            self.report_finished("cancelled", None);
        }
    }
}
