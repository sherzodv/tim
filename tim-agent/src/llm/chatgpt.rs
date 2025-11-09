use async_trait::async_trait;

use super::{Llm, LlmError, LlmReq, LlmRes};

#[derive(Debug, Default)]
pub struct ChatGpt;

impl ChatGpt {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Llm for ChatGpt {
    async fn chat(&self, req: &LlmReq<'_>) -> Result<LlmRes, LlmError> {
        if req.msg.trim().is_empty() {
            return Err(LlmError::EmptyPrompt);
        }

        let reply = format!(
            "[sysp: {sysp}] {userp}: {msg}",
            sysp = req.sysp,
            userp = req.userp,
            msg = req.msg
        );

        Ok(LlmRes { message: reply })
    }
}
