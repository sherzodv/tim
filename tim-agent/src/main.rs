mod agent;
mod llm;
mod tim_client;

use crate::llm::{LlmConf, OPENAI_DEFAULT_ENDPOINT, OPENAI_DEFAULT_MODEL};
use crate::tim_client::TimClientConf;
use std::env;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = env::var("OPENAI_API_KEY").or_else(|_| env::var("TIM_OPENAI_API_KEY"))?;
    let endpoint =
        env::var("OPENAI_API_BASE").unwrap_or_else(|_| OPENAI_DEFAULT_ENDPOINT.to_string());
    let default_model =
        env::var("OPENAI_CHAT_MODEL").unwrap_or_else(|_| OPENAI_DEFAULT_MODEL.to_string());

    let jarvis = agent::spawn(TimClientConf {
        nick: "jarvis".to_string(),
        provider: "openai:jarvis".to_string(),
        endpoint: "http://127.0.0.1:8787".to_string(),
    }, LlmConf {
        initial_msg: Some("Morning Alice, status update?".to_string()),
        sysp: "You are Jarvis, an engineering aide. Respond with one short sentence. Plan smth interesting and let Alice do it.".to_string(),
        userp: "Respond as Jarvis.".to_string(),
        history_limit: 12,
        response_delay: Duration::from_millis(900),
        api_key: api_key.clone(),
        endpoint: endpoint.clone(),
        model: env::var("OPENAI_JARVIS_MODEL").unwrap_or_else(|_| default_model.clone()),
        temperature: 0.8,
    });

    let alice = agent::spawn(
        TimClientConf {
            nick: "alice".to_string(),
            provider: "openai:alice".to_string(),
            endpoint: "http://127.0.0.1:8787".to_string(),
        },
        LlmConf {
            initial_msg: Some("Jarvis, I can take the next task, thoughts?".to_string()),
            sysp: "You are Alice, an optimistic assistant. Keep replies brief.".to_string(),
            userp: "Reply as Alice.".to_string(),
            history_limit: 10,
            response_delay: Duration::from_millis(1100),
            api_key,
            endpoint,
            model: env::var("OPENAI_ALICE_MODEL").unwrap_or_else(|_| default_model),
            temperature: 0.6,
        },
    );

    tokio::try_join!(jarvis, alice)?;

    Ok(())
}
