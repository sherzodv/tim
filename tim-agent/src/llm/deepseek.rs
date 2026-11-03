//! DeepSeek over its OpenAI-compatible Responses endpoint.

use async_trait::async_trait;
use serde::Serialize;

use super::llm::Llm;
use super::llm::LlmConf;
use super::llm::LlmError;
use super::llm::LlmReq;
use super::llm::LlmStream;
use super::responses::ResponsesClient;
use super::responses::ResponsesReq;

pub const DEEPSEEK_DEFAULT_ENDPOINT: &str = "https://api.deepseek.com/v1/responses";
pub const DEEPSEEK_DEFAULT_MODEL: &str = "deepseek-v4-pro";

/// Thinking stays off: with it on, DeepSeek rejects any request whose past assistant
/// turns come without their reasoning, and the agent does not keep reasoning yet.
const REASONING_EFFORT: &str = "none";

#[derive(Clone, Debug)]
pub struct DeepSeek {
    client: ResponsesClient,
}

#[derive(Serialize, Debug)]
struct DeepSeekReq {
    #[serde(flatten)]
    base: ResponsesReq,
    reasoning: Reasoning,
}

#[derive(Serialize, Debug)]
struct Reasoning {
    effort: &'static str,
}

impl DeepSeek {
    pub fn new(conf: &LlmConf) -> Result<Self, LlmError> {
        let client = ResponsesClient::new(conf, DEEPSEEK_DEFAULT_ENDPOINT, DEEPSEEK_DEFAULT_MODEL)?;
        Ok(Self { client })
    }
}

#[async_trait]
impl Llm for DeepSeek {
    async fn chat_stream(&self, req: &LlmReq<'_>) -> Result<LlmStream, LlmError> {
        let payload = DeepSeekReq {
            base: self.client.request(req)?,
            reasoning: Reasoning {
                effort: REASONING_EFFORT,
            },
        };
        self.client.stream(&payload).await
    }
}
