mod gpt;

use gpt::{
    ChatGptClient, GptChatRequest, GptClient, GptGenerationControls, GptMessage, GptMessageRole,
};
use std::error::Error;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    run().await
}

async fn run() -> Result<(), Box<dyn Error>> {
    let api_key = std::env::var("OPENAI_TIM_API_KEY")
        .map_err(|_| "set OPENAI_TIM_API_KEY environment variable to run chatgtptest")?;

    let model = std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string());

    let mut controls = GptGenerationControls::new();
    controls.max_output_tokens = Some(160);
    controls.temperature = Some(0.2);
    controls.timeout = Some(Duration::from_secs(30));

    let messages = vec![
        GptMessage {
            role: GptMessageRole::System,
            content: "You are Tim, a command centric assistant.".to_string(),
        },
        GptMessage {
            role: GptMessageRole::User,
            content: "Say hello and describe the interface in one short sentence.".to_string(),
        },
    ];

    let request = GptChatRequest::new(model, messages).with_controls(controls);

    let client = ChatGptClient::new(api_key)?;
    let response = client.chat(request).await?;

    if let Some(choice) = response.choices.first() {
        println!("assistant:\n{}", choice.message.content);
    } else {
        println!("assistant: <no response>");
    }

    if let Some(usage) = response.usage {
        println!(
            "\nusage: prompt={} completion={} total={}",
            usage.prompt_tokens, usage.completion_tokens, usage.total_tokens
        );
    }

    if let Some(request_id) = response.provider_request_id {
        println!("\nprovider request id: {request_id}");
    }

    Ok(())
}
