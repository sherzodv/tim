pub mod chatgpt;

pub use chatgpt::ChatGpt;

use async_trait::async_trait;

pub struct LlmReq<'a> {
    pub sysp: &'a str,
    pub userp: &'a str,
    pub msg: &'a str,
}

pub struct LlmRes {
    pub message: String,
}

#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("llm prompt is empty")]
    EmptyPrompt,
    #[error("missing OpenAI API key (set OPENAI_API_KEY)")]
    MissingApiKey,
    #[error("http error while contacting LLM: {0}")]
    Http(#[from] reqwest::Error),
    #[error("failed to decode LLM response: {0}")]
    Response(#[from] serde_json::Error),
    #[error("LLM reported an error: {0}")]
    Api(String),
    #[error("LLM response missing message content")]
    MissingContent,
}

#[async_trait]
pub trait Llm: Send + Sync {
    async fn chat(&self, req: &LlmReq<'_>) -> Result<LlmRes, LlmError>;
}
