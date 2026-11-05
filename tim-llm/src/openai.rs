//! OpenAI over its Responses API.

use async_trait::async_trait;

use crate::llm::Llm;
use crate::llm::LlmConf;
use crate::llm::LlmError;
use crate::llm::LlmReq;
use crate::llm::LlmStream;
use crate::responses::ResponsesClient;

pub const OPENAI_DEFAULT_ENDPOINT: &str = "https://api.openai.com/v1/responses";
pub const OPENAI_DEFAULT_MODEL: &str = "gpt-4.1";

#[derive(Clone, Debug)]
pub struct OpenAi {
    client: ResponsesClient,
}

impl OpenAi {
    pub fn new(conf: &LlmConf) -> Result<Self, LlmError> {
        let client = ResponsesClient::new(conf, OPENAI_DEFAULT_ENDPOINT, OPENAI_DEFAULT_MODEL)?;
        Ok(Self { client })
    }
}

#[async_trait]
impl Llm for OpenAi {
    async fn chat_stream(&self, req: &LlmReq<'_>) -> Result<LlmStream, LlmError> {
        let payload = self.client.request(req)?;
        self.client.stream(&payload).await
    }
}
