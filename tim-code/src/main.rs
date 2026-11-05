mod claude;
mod code;
mod fs;
mod git;
mod history;
mod llm;
mod progress;
mod shell;
mod text;
mod tools;

use std::path::PathBuf;
use std::time::Duration;

use config::Config;
use config::File;
use config::FileFormat;
use dotenvy::dotenv;
use serde::Deserialize;
use shellexpand::env_with_context_no_errors as expand_env;
use tim_client::agent;
use tim_client::client::TimClientConf;
use tim_llm::llm::LlmConf;
use tracing_subscriber::fmt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

use crate::code::CodeConf;
use crate::code::EngineConf;
use crate::tools::Tools;

const CONFIG_PATH: &str = "code.toml";

#[derive(Deserialize)]
struct AppConfig {
    username: String,
    provider: String,
    endpoint: String,
    workdir: Option<PathBuf>,
    timeout_secs: u64,
    max_output_chars: usize,
    #[serde(flatten)]
    engine: EngineConfig,
}

#[derive(Deserialize)]
#[serde(tag = "engine", rename_all = "lowercase")]
enum EngineConfig {
    Claude { claude_allowed_tools: Vec<String> },
    Llm { llm: LlmConfig },
}

#[derive(Deserialize)]
struct LlmConfig {
    provider: String,
    api_key: String,
    endpoint: Option<String>,
    model: String,
    temperature: f32,
    reasoning: Option<String>,
    max_steps: usize,
}

fn load_config() -> Result<AppConfig, Box<dyn std::error::Error>> {
    dotenv().ok();

    let raw = std::fs::read_to_string(CONFIG_PATH)?;
    // A missing variable becomes empty: the engine that needs it reports it, the other runs.
    let expanded = expand_env(&raw, |name| Some(std::env::var(name).unwrap_or_default()));

    let config = Config::builder()
        .add_source(File::from_str(&expanded, FileFormat::Toml))
        .build()?
        .try_deserialize()?;

    Ok(config)
}

fn engine_conf(config: EngineConfig) -> EngineConf {
    match config {
        EngineConfig::Claude {
            claude_allowed_tools,
        } => EngineConf::Claude {
            allowed_tools: claude_allowed_tools,
        },
        EngineConfig::Llm { llm } => EngineConf::Llm {
            provider: llm.provider,
            conf: LlmConf {
                api_key: llm.api_key,
                endpoint: llm.endpoint,
                model: llm.model,
                temperature: llm.temperature,
                reasoning: llm.reasoning,
            },
            max_steps: llm.max_steps,
        },
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env())
        .with(fmt::layer())
        .init();

    let config = load_config()?;

    let tim_conf = TimClientConf {
        endpoint: config.endpoint,
        username: config.username,
        provider: config.provider,
    };

    let workdir = config.workdir.map_or_else(std::env::current_dir, Ok)?;
    let code_conf = CodeConf {
        tools: Tools::new(
            workdir,
            Duration::from_secs(config.timeout_secs),
            config.max_output_chars,
        ),
        engine: engine_conf(config.engine),
    };

    agent::spawn(tim_conf, code_conf).await?;
    Ok(())
}
