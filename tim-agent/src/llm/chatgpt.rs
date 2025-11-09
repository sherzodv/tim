use std::env;
use std::fmt;

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::{Llm, LlmError, LlmReq, LlmRes};

const DEFAULT_OPENAI_URL: &str = "https://api.openai.com/v1/chat/completions";
const DEFAULT_OPENAI_MODEL: &str = "gpt-4o-mini";

#[derive(Clone)]
pub struct ChatGpt {
    client: Client,
    api_key: String,
    endpoint: String,
    model: String,
}

impl fmt::Debug for ChatGpt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChatGpt")
            .field("endpoint", &self.endpoint)
            .field("model", &self.model)
            .finish()
    }
}

impl ChatGpt {
    pub fn new() -> Result<Self, LlmError> {
        let api_key = env::var("OPENAI_API_KEY")
            .or_else(|_| env::var("TIM_OPENAI_API_KEY"))
            .map_err(|_| LlmError::MissingApiKey)?;
        let endpoint =
            env::var("OPENAI_API_BASE").unwrap_or_else(|_| DEFAULT_OPENAI_URL.to_string());
        let model =
            env::var("OPENAI_CHAT_MODEL").unwrap_or_else(|_| DEFAULT_OPENAI_MODEL.to_string());

        Ok(Self {
            client: Client::new(),
            api_key,
            endpoint,
            model,
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
            temperature: 0.7,
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
