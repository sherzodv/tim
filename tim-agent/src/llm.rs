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
}

#[async_trait]
pub trait Llm: Send + Sync {
    async fn chat(&self, req: &LlmReq<'_>) -> Result<LlmRes, LlmError>;
}
