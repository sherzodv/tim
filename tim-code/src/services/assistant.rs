use std::time::Duration;

use crate::gpt::chatgpt::ChatGptClient;
use crate::gpt::{
    GptChatRequest, GptClient, GptClientError, GptGenerationControls, GptMessage, GptMessageRole,
    GptUsage,
};

#[derive(Clone)]
pub struct ChatBridge {
    client: ChatGptClient,
    model: String,
    system_prompt: String,
    max_output_tokens: Option<u32>,
    temperature: f32,
    timeout: Duration,
}

impl ChatBridge {
    pub fn from_env() -> Result<Self, ChatBridgeInitError> {
        let api_key =
            std::env::var("OPENAI_TIM_API_KEY").map_err(|_| ChatBridgeInitError::MissingApiKey)?;
        let model = std::env::var("OPENAI_TIM_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string());
        let system_prompt = std::env::var("OPENAI_TIM_SYSTEM_PROMPT")
            .unwrap_or_else(|_| "You are Tim, a command centric assistant.".to_string());
        let client = ChatGptClient::new(api_key).map_err(|err| {
            ChatBridgeInitError::Client(format!("failed to create ChatGPT client: {err}"))
        })?;

        Ok(Self {
            client,
            model,
            system_prompt,
            max_output_tokens: Some(320),
            temperature: 0.2,
            timeout: Duration::from_secs(30),
        })
    }

    pub fn model_name(&self) -> &str {
        &self.model
    }

    pub async fn send(&self, user_command: &str) -> Result<ChatBridgeReply, GptClientError> {
        let mut controls = GptGenerationControls::new();
        controls.max_output_tokens = self.max_output_tokens;
        controls.temperature = Some(self.temperature);
        controls.timeout = Some(self.timeout);

        let messages = vec![
            GptMessage {
                role: GptMessageRole::System,
                content: self.system_prompt.clone(),
            },
            GptMessage {
                role: GptMessageRole::User,
                content: user_command.to_string(),
            },
        ];

        let request = GptChatRequest::new(self.model.clone(), messages).with_controls(controls);
        let response = self.client.chat(request).await?;
        let text = response
            .choices
            .first()
            .map(|choice| choice.message.content.clone())
            .filter(|content| !content.trim().is_empty())
            .ok_or_else(|| {
                GptClientError::Provider("assistant returned an empty response".to_string())
            })?;

        Ok(ChatBridgeReply {
            text,
            usage: response.usage,
            provider_request_id: response.provider_request_id,
        })
    }
}

#[derive(Debug)]
pub enum ChatBridgeInitError {
    MissingApiKey,
    Client(String),
}

impl std::fmt::Display for ChatBridgeInitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChatBridgeInitError::MissingApiKey => {
                write!(f, "OPENAI_TIM_API_KEY environment variable is not set")
            }
            ChatBridgeInitError::Client(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for ChatBridgeInitError {}

pub struct ChatBridgeReply {
    pub text: String,
    pub usage: Option<GptUsage>,
    pub provider_request_id: Option<String>,
}
