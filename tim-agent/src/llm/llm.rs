use async_trait::async_trait;
use thiserror::Error;

pub(super) struct LlmReq<'a> {
    pub(super) sysp: &'a str,
    pub(super) userp: &'a str,
    pub(super) msg: &'a str,
}

pub(super) struct LlmRes {
    pub(super) message: String,
}

#[derive(Debug, Error)]
pub(super) enum LlmError {
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
pub(super) trait Llm: Send + Sync {
    async fn chat(&self, req: &LlmReq<'_>) -> Result<LlmRes, LlmError>;
}
