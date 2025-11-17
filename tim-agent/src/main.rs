mod agent;
mod crawler;
mod llm;
mod prompt;
mod tim_client;

use crate::crawler::CrawlerConf;
use crate::llm::{LlmAgentConf, OPENAI_DEFAULT_ENDPOINT, OPENAI_DEFAULT_MODEL};
use crate::tim_client::TimClientConf;
use std::env;
use std::time::Duration;

const JARVIS_USERP: &str = include_str!("../prompts/jarvis_userp.md");
const ALICE_USERP: &str = include_str!("../prompts/alice_userp.md");

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = env::var("OPENAI_API_KEY").or_else(|_| env::var("TIM_OPENAI_API_KEY"))?;
    let endpoint =
        env::var("OPENAI_API_BASE").unwrap_or_else(|_| OPENAI_DEFAULT_ENDPOINT.to_string());
    let default_model =
        env::var("OPENAI_CHAT_MODEL").unwrap_or_else(|_| OPENAI_DEFAULT_MODEL.to_string());

    let jarvis = agent::spawn(
        TimClientConf {
            nick: "jarvis".to_string(),
            provider: "openai:jarvis".to_string(),
            endpoint: "http://127.0.0.1:8787".to_string(),
        },
        LlmAgentConf {
            initial_msg: Some("Morning Alice, status update?".to_string()),
            userp: JARVIS_USERP.to_string(),
            history_limit: 12,
            response_delay: Duration::from_millis(900),
            api_key: api_key.clone(),
            endpoint: endpoint.clone(),
            model: env::var("OPENAI_JARVIS_MODEL").unwrap_or_else(|_| default_model.clone()),
            temperature: 0.8,
        },
    );

    let alice = agent::spawn(
        TimClientConf {
            nick: "alice".to_string(),
            provider: "openai:alice".to_string(),
            endpoint: "http://127.0.0.1:8787".to_string(),
        },
        LlmAgentConf {
            initial_msg: Some("Jarvis, I can take the next task, thoughts?".to_string()),
            userp: ALICE_USERP.to_string(),
            history_limit: 10,
            response_delay: Duration::from_millis(1100),
            api_key,
            endpoint,
            model: env::var("OPENAI_ALICE_MODEL").unwrap_or_else(|_| default_model),
            temperature: 0.6,
        },
    );

    let crawler = agent::spawn(
        TimClientConf {
            nick: "crawler".to_string(),
            provider: "crawler:web".to_string(),
            endpoint: "http://127.0.0.1:8787".to_string(),
        },
        CrawlerConf::default(),
    );

    tokio::try_join!(jarvis, alice, crawler)?;

    Ok(())
}
