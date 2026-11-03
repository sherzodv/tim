use std::env;

use futures::StreamExt;
use tim_agent::llm::llm::Llm;
use tim_agent::llm::llm::LlmConf;
use tim_agent::llm::llm::LlmInputItem;
use tim_agent::llm::llm::LlmReq;
use tim_agent::llm::llm::LlmRole;
use tim_agent::llm::llm::LlmStreamEvent;
use tim_agent::llm::openai::OpenAi;
use tim_agent::llm::openai::OPENAI_DEFAULT_ENDPOINT;
use tim_agent::llm::openai::OPENAI_DEFAULT_MODEL;
use tracing_subscriber::EnvFilter;

// Manual streaming test. Run with: cargo test --test sse_debug -- --ignored
#[tokio::test]
#[ignore = "requires OpenAI key and network"]
async fn stream_debug() -> Result<(), Box<dyn std::error::Error>> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "debug".into()))
        .try_init();

    let api_key = match env::var("OPENAI_API_KEY").or_else(|_| env::var("TIM_OPENAI_API_KEY")) {
        Ok(key) => key,
        Err(_) => return Ok(()), // skip if no key
    };

    let endpoint =
        env::var("OPENAI_API_BASE").unwrap_or_else(|_| OPENAI_DEFAULT_ENDPOINT.to_string());
    let model = env::var("OPENAI_CHAT_MODEL").unwrap_or_else(|_| OPENAI_DEFAULT_MODEL.to_string());

    let prompt = env::var("TIM_SSE_DEBUG_PROMPT").unwrap_or_else(|_| {
        "Reply with some brief greeting first, then include a simple tool call".to_string()
    });

    let openai = OpenAi::new(&LlmConf {
        api_key,
        endpoint: Some(endpoint.clone()),
        model: model.clone(),
        temperature: 0.2,
    })?;

    let history = vec![LlmInputItem {
        role: LlmRole::User,
        content: prompt,
    }];
    let req = LlmReq {
        sysp: "You are a test harness. If tools are provided, call them.",
        inputs: &history,
    };

    let mut stream = openai.chat_stream(&req).await?;
    println!("starting stream model={model} endpoint={endpoint}");

    while let Some(item) = stream.next().await {
        match item? {
            LlmStreamEvent::ContentDelta(delta) => println!("delta: {delta}"),
            LlmStreamEvent::ToolCall { name, arguments } => {
                println!("tool_call name={name} args={arguments}")
            }
            LlmStreamEvent::Completed => {
                println!("completed");
                break;
            }
        }
    }

    Ok(())
}
