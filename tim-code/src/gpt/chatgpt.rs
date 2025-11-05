use std::time::Duration;

use async_trait::async_trait;
use reqwest::{Client as HttpClient, StatusCode};
use serde::{Deserialize, Serialize};

use super::{
    GptChatChoice, GptChatRequest, GptChatResponse, GptClient, GptClientError, GptClientResult,
    GptCompletionChoice, GptCompletionRequest, GptCompletionResponse, GptGenerationControls,
    GptMessage, GptMessageRole, GptUsage,
};

const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";
const DEFAULT_USER_AGENT: &str = "tim-code-chatgpt-client";
const CHAT_PATH: &str = "chat/completions";
const COMPLETION_PATH: &str = "completions";

/// Builder for configuring a [`ChatGptClient`].
pub struct ChatGptClientBuilder {
    api_key: String,
    base_url: String,
    organization: Option<String>,
    http_timeout: Option<Duration>,
    user_agent: String,
}

impl ChatGptClientBuilder {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: DEFAULT_BASE_URL.to_string(),
            organization: None,
            http_timeout: None,
            user_agent: DEFAULT_USER_AGENT.to_string(),
        }
    }

    pub fn base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    pub fn organization(mut self, organization: impl Into<String>) -> Self {
        self.organization = Some(organization.into());
        self
    }

    pub fn http_timeout(mut self, timeout: Duration) -> Self {
        self.http_timeout = Some(timeout);
        self
    }

    pub fn user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.user_agent = user_agent.into();
        self
    }

    pub fn build(self) -> GptClientResult<ChatGptClient> {
        let mut http_builder = HttpClient::builder().user_agent(self.user_agent);
        if let Some(timeout) = self.http_timeout {
            http_builder = http_builder.timeout(timeout);
        }

        let http = http_builder
            .build()
            .map_err(|err| GptClientError::Transport(err.to_string()))?;

        Ok(ChatGptClient {
            http,
            base_url: self.base_url.trim_end_matches('/').to_string(),
            api_key: self.api_key,
            organization: self.organization,
        })
    }
}

#[derive(Clone)]
pub struct ChatGptClient {
    http: HttpClient,
    base_url: String,
    api_key: String,
    organization: Option<String>,
}

impl ChatGptClient {
    pub fn builder(api_key: impl Into<String>) -> ChatGptClientBuilder {
        ChatGptClientBuilder::new(api_key)
    }

    pub fn new(api_key: impl Into<String>) -> GptClientResult<Self> {
        Self::builder(api_key).build()
    }

    fn join_path(&self, path: &str) -> String {
        format!(
            "{}/{}",
            self.base_url,
            path.trim_start_matches('/').trim_end_matches('/')
        )
    }

    fn prepare_post(&self, path: &str) -> reqwest::RequestBuilder {
        let url = self.join_path(path);
        let builder = self.http.post(url).bearer_auth(&self.api_key);
        if let Some(org) = &self.organization {
            builder.header("OpenAI-Organization", org)
        } else {
            builder
        }
    }
}

#[async_trait]
impl GptClient for ChatGptClient {
    fn provider_name(&self) -> &'static str {
        "openai-chatgpt"
    }

    async fn chat(&self, request: GptChatRequest) -> GptClientResult<GptChatResponse> {
        let GptChatRequest {
            model,
            messages,
            controls,
        } = request;

        if messages.is_empty() {
            return Err(GptClientError::InvalidRequest(
                "chat request requires at least one message".to_string(),
            ));
        }

        let GptGenerationControls {
            max_output_tokens,
            temperature,
            top_p,
            timeout,
        } = controls;

        let payload = OpenAiChatRequest {
            model,
            messages: messages.into_iter().map(OpenAiChatMessage::from).collect(),
            max_tokens: max_output_tokens,
            temperature,
            top_p,
        };

        let builder = self.prepare_post(CHAT_PATH).json(&payload);
        let builder = apply_timeout(builder, timeout);
        let response = builder
            .send()
            .await
            .map_err(|err| GptClientError::Transport(err.to_string()))?;

        handle_response(response, |parsed: OpenAiChatResponse| {
            let choices = parsed
                .choices
                .into_iter()
                .map(|choice| GptChatChoice {
                    message: GptMessage {
                        role: map_role(choice.message.role),
                        content: choice.message.content,
                    },
                    finish_reason: choice.finish_reason,
                })
                .collect();
            Ok(GptChatResponse {
                choices,
                usage: map_usage(parsed.usage),
                provider_request_id: Some(parsed.id),
            })
        })
        .await
    }

    async fn completion(
        &self,
        request: GptCompletionRequest,
    ) -> GptClientResult<GptCompletionResponse> {
        let GptCompletionRequest {
            model,
            prompt,
            suffix,
            controls,
        } = request;

        if prompt.trim().is_empty() {
            return Err(GptClientError::InvalidRequest(
                "completion prompt cannot be empty".to_string(),
            ));
        }

        let GptGenerationControls {
            max_output_tokens,
            temperature,
            top_p,
            timeout,
        } = controls;

        let payload = OpenAiCompletionRequest {
            model,
            prompt,
            suffix,
            max_tokens: max_output_tokens,
            temperature,
            top_p,
        };

        let builder = self.prepare_post(COMPLETION_PATH).json(&payload);
        let builder = apply_timeout(builder, timeout);
        let response = builder
            .send()
            .await
            .map_err(|err| GptClientError::Transport(err.to_string()))?;

        handle_response(response, |parsed: OpenAiCompletionResponse| {
            let choices = parsed
                .choices
                .into_iter()
                .map(|choice| GptCompletionChoice {
                    text: choice.text,
                    finish_reason: choice.finish_reason,
                })
                .collect();
            Ok(GptCompletionResponse {
                choices,
                usage: map_usage(parsed.usage),
                provider_request_id: Some(parsed.id),
            })
        })
        .await
    }
}

fn apply_timeout(
    builder: reqwest::RequestBuilder,
    timeout: Option<Duration>,
) -> reqwest::RequestBuilder {
    if let Some(timeout) = timeout {
        builder.timeout(timeout)
    } else {
        builder
    }
}

async fn handle_response<T, F, R>(response: reqwest::Response, map: F) -> GptClientResult<R>
where
    T: for<'de> Deserialize<'de>,
    F: FnOnce(T) -> GptClientResult<R>,
{
    let status = response.status();

    if !status.is_success() {
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "<unavailable>".to_string());
        return Err(provider_error(status, &body));
    }

    let payload = response
        .json::<T>()
        .await
        .map_err(|err| GptClientError::Transport(err.to_string()))?;

    map(payload)
}

fn map_usage(usage: Option<OpenAiUsage>) -> Option<GptUsage> {
    usage.map(|usage| GptUsage {
        prompt_tokens: usage.prompt_tokens,
        completion_tokens: usage.completion_tokens,
        total_tokens: usage.total_tokens,
    })
}

fn map_role(value: String) -> GptMessageRole {
    match value.as_str() {
        "system" => GptMessageRole::System,
        "user" => GptMessageRole::User,
        "assistant" => GptMessageRole::Assistant,
        "tool" => GptMessageRole::Tool,
        _ => GptMessageRole::Assistant,
    }
}

fn provider_error(status: StatusCode, body: &str) -> GptClientError {
    if let Ok(parsed) = serde_json::from_str::<OpenAiErrorEnvelope>(body) {
        let mut message = parsed.error.message;
        if let Some(error_type) = parsed.error.r#type {
            message = format!("{message} (type: {error_type})");
        }
        GptClientError::Provider(format!("{}: {}", status.as_u16(), message))
    } else {
        GptClientError::Provider(format!("{}: {}", status.as_u16(), body.trim().to_string()))
    }
}

#[derive(Serialize)]
struct OpenAiChatRequest {
    model: String,
    messages: Vec<OpenAiChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
}

#[derive(Serialize, Deserialize)]
struct OpenAiChatMessage {
    role: String,
    content: String,
}

impl From<GptMessage> for OpenAiChatMessage {
    fn from(message: GptMessage) -> Self {
        Self {
            role: message.role.as_str().to_string(),
            content: message.content,
        }
    }
}

#[derive(Deserialize)]
struct OpenAiChatResponse {
    id: String,
    choices: Vec<OpenAiChatChoicePayload>,
    usage: Option<OpenAiUsage>,
}

#[derive(Deserialize)]
struct OpenAiChatChoicePayload {
    message: OpenAiChatMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Serialize)]
struct OpenAiCompletionRequest {
    model: String,
    prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    suffix: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
}

#[derive(Deserialize)]
struct OpenAiCompletionResponse {
    id: String,
    choices: Vec<OpenAiCompletionChoicePayload>,
    usage: Option<OpenAiUsage>,
}

#[derive(Deserialize)]
struct OpenAiCompletionChoicePayload {
    text: String,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct OpenAiUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

#[derive(Deserialize)]
struct OpenAiErrorEnvelope {
    error: OpenAiErrorBody,
}

#[derive(Deserialize)]
struct OpenAiErrorBody {
    message: String,
    #[serde(default)]
    r#type: Option<String>,
}
