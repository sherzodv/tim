//! DeepSeek over its OpenAI-compatible Responses endpoint.

use async_trait::async_trait;
use serde::Serialize;

use crate::llm::Llm;
use crate::llm::LlmConf;
use crate::llm::LlmError;
use crate::llm::LlmReq;
use crate::llm::LlmStream;
use crate::responses::ResponsesClient;
use crate::responses::ResponsesReq;

pub const DEEPSEEK_DEFAULT_ENDPOINT: &str = "https://api.deepseek.com/v1/responses";
pub const DEEPSEEK_DEFAULT_MODEL: &str = "deepseek-v4-pro";

/// Thinking is off unless configured: with it on, DeepSeek rejects any request whose
/// past turns come without their reasoning, so callers must pass turns back verbatim.
const DEFAULT_REASONING_EFFORT: &str = "none";

#[derive(Clone, Debug)]
pub struct DeepSeek {
    client: ResponsesClient,
    reasoning_effort: String,
}

#[derive(Serialize, Debug)]
struct DeepSeekReq {
    #[serde(flatten)]
    base: ResponsesReq,
    reasoning: Reasoning,
}

#[derive(Serialize, Debug)]
struct Reasoning {
    effort: String,
}

impl DeepSeek {
    pub fn new(conf: &LlmConf) -> Result<Self, LlmError> {
        let client = ResponsesClient::new(conf, DEEPSEEK_DEFAULT_ENDPOINT, DEEPSEEK_DEFAULT_MODEL)?;
        let reasoning_effort = conf
            .reasoning
            .clone()
            .unwrap_or_else(|| DEFAULT_REASONING_EFFORT.to_string());
        Ok(Self {
            client,
            reasoning_effort,
        })
    }
}

#[async_trait]
impl Llm for DeepSeek {
    async fn chat_stream(&self, req: &LlmReq<'_>) -> Result<LlmStream, LlmError> {
        let payload = DeepSeekReq {
            base: self.client.request(req)?,
            reasoning: Reasoning {
                effort: self.reasoning_effort.clone(),
            },
        };
        self.client.stream(&payload).await
    }
}
