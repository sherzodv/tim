mod agent;
mod llm;
mod prompt;
mod tim_client;

use crate::llm::{LlmConf, OPENAI_DEFAULT_ENDPOINT, OPENAI_DEFAULT_MODEL};
use crate::prompt::render as render_prompt;
use crate::tim_client::TimClientConf;
use serde::Serialize;
use std::env;
use std::time::Duration;

const JARVIS_SYSP_TEMPLATE: &str = include_str!("../prompts/jarvis_sysp.txt");
const JARVIS_USERP_TEMPLATE: &str = include_str!("../prompts/jarvis_userp.txt");
const ALICE_SYSP_TEMPLATE: &str = include_str!("../prompts/alice_sysp.txt");
const ALICE_USERP_TEMPLATE: &str = include_str!("../prompts/alice_userp.txt");

#[derive(Serialize)]
struct PromptCtx<'a> {
    agent: &'a str,
    peer: &'a str,
}

impl<'a> PromptCtx<'a> {
    fn new(agent: &'a str, peer: &'a str) -> Self {
        Self { agent, peer }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = env::var("OPENAI_API_KEY").or_else(|_| env::var("TIM_OPENAI_API_KEY"))?;
    let endpoint =
        env::var("OPENAI_API_BASE").unwrap_or_else(|_| OPENAI_DEFAULT_ENDPOINT.to_string());
    let default_model =
        env::var("OPENAI_CHAT_MODEL").unwrap_or_else(|_| OPENAI_DEFAULT_MODEL.to_string());

    let jarvis_ctx = PromptCtx::new("Jarvis", "Alice");
    let jarvis_sysp = render_prompt(JARVIS_SYSP_TEMPLATE, &jarvis_ctx)?;
    let jarvis_userp = render_prompt(JARVIS_USERP_TEMPLATE, &jarvis_ctx)?;
    let alice_ctx = PromptCtx::new("Alice", "Jarvis");
    let alice_sysp = render_prompt(ALICE_SYSP_TEMPLATE, &alice_ctx)?;
    let alice_userp = render_prompt(ALICE_USERP_TEMPLATE, &alice_ctx)?;

    let jarvis = agent::spawn(
        TimClientConf {
            nick: "jarvis".to_string(),
            provider: "openai:jarvis".to_string(),
            endpoint: "http://127.0.0.1:8787".to_string(),
        },
        LlmConf {
            initial_msg: Some("Morning Alice, status update?".to_string()),
            sysp: jarvis_sysp,
            userp: jarvis_userp,
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
        LlmConf {
            initial_msg: Some("Jarvis, I can take the next task, thoughts?".to_string()),
            sysp: alice_sysp,
            userp: alice_userp,
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
