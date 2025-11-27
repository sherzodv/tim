mod agent;
mod crawler;
mod llm;
mod tim_client;

use std::env;
use std::time::Duration;

use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use crate::crawler::CrawlerConf;
use crate::llm::AgentConf;
use crate::llm::OPENAI_DEFAULT_ENDPOINT;
use crate::llm::OPENAI_DEFAULT_MODEL;
use crate::tim_client::TimClientConf;

const JARVIS_USERP: &str = include_str!("../prompts/jarvis_userp.md");
const ALICE_USERP: &str = include_str!("../prompts/alice_userp.md");
const JARVIS_LIVE_INTERVAL: Duration = Duration::from_secs(100);
const ALICE_LIVE_INTERVAL: Duration = Duration::from_secs(100);

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env())
        .with(fmt::layer())
        .init();

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
        AgentConf {
            userp: JARVIS_USERP.to_string(),
            api_key: api_key.clone(),
            endpoint: endpoint.clone(),
            model: env::var("OPENAI_JARVIS_MODEL").unwrap_or_else(|_| default_model.clone()),
            temperature: 0.8,
            live_interval: Some(JARVIS_LIVE_INTERVAL),
        },
    );

    let alice = agent::spawn(
        TimClientConf {
            nick: "alice".to_string(),
            provider: "openai:alice".to_string(),
            endpoint: "http://127.0.0.1:8787".to_string(),
        },
        AgentConf {
            userp: ALICE_USERP.to_string(),
            api_key,
            endpoint,
            model: env::var("OPENAI_ALICE_MODEL").unwrap_or_else(|_| default_model),
            temperature: 0.6,
            live_interval: Some(ALICE_LIVE_INTERVAL),
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
