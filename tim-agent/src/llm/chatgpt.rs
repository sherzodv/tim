use std::fmt;

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::{Llm, LlmConf, LlmError, LlmReq, LlmRes};

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
    pub fn new(cfg: LlmConf) -> Result<Self, LlmError> {
        if cfg.api_key.trim().is_empty() {
            return Err(LlmError::MissingApiKey);
        }
        let endpoint = if cfg.endpoint.trim().is_empty() {
            OPENAI_DEFAULT_ENDPOINT.to_string()
        } else {
            cfg.endpoint
        };
        let model = if cfg.model.trim().is_empty() {
            OPENAI_DEFAULT_MODEL.to_string()
        } else {
            cfg.model
        };

        Ok(Self {
            client: Client::new(),
            api_key: cfg.api_key,
            endpoint,
            model,
            temperature: cfg.temperature.max(0.0),
        })
    }
}

#[derive(Serialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
}

#[derive(Serialize)]
struct ChatMessage {
    role: &'static str,
    content: String,
}

#[derive(Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: CompletionMessage,
}

#[derive(Deserialize)]
struct CompletionMessage {
    content: Option<String>,
}

#[async_trait]
impl Llm for ChatGpt {
    async fn chat(&self, req: &LlmReq<'_>) -> Result<LlmRes, LlmError> {
        if req.msg.trim().is_empty() {
            return Err(LlmError::EmptyPrompt);
        }

        let payload = ChatCompletionRequest {
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
        };

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

        let completion: ChatCompletionResponse = response.json().await?;
        let message = completion
            .choices
            .into_iter()
            .find_map(|choice| choice.message.content)
            .ok_or(LlmError::MissingContent)?;

        Ok(LlmRes {
            message: message.trim().to_string(),
        })
    }
}
