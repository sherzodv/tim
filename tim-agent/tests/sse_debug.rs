use std::env;

use futures::StreamExt;
use tim_agent::llm::chatgpt::ChatGpt;
use tim_agent::llm::chatgpt::OPENAI_DEFAULT_ENDPOINT;
use tim_agent::llm::chatgpt::OPENAI_DEFAULT_MODEL;
use tim_agent::llm::llm::Llm;
use tim_agent::llm::llm::LlmReq;
use tim_agent::llm::llm::LlmStreamEvent;
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

    let chatgpt = ChatGpt::new(api_key, endpoint.clone(), model.clone(), 0.2)?;

    let req = LlmReq {
        sysp: "You are a test harness. If tools are provided, call them.",
        userp: "",
        msg: &prompt,
    };

    let mut stream = chatgpt.chat_stream(&req).await?;
    println!("starting stream model={model} endpoint={endpoint}");

    while let Some(item) = stream.next().await {
        match item? {
            LlmStreamEvent::ContentDelta(delta) => println!("delta: {delta}"),
            LlmStreamEvent::ToolCallDelta {
                id,
                name,
                arguments_delta,
                finished,
            } => println!(
                "tool_call id={id} name={:?} args_delta={arguments_delta} finished={finished}",
                name
            ),
            LlmStreamEvent::Completed => {
                println!("completed");
                break;
            }
        }
    }

    Ok(())
}
