mod claude;
mod code;
mod fs;
mod git;
mod shell;
mod text;

use std::path::PathBuf;
use std::time::Duration;

use config::Config;
use config::ConfigError;
use config::File;
use config::FileFormat;
use serde::Deserialize;
use tim_client::agent;
use tim_client::client::TimClientConf;
use tracing_subscriber::fmt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

use crate::code::CodeConf;

const CONFIG_PATH: &str = "code.toml";

#[derive(Deserialize)]
struct AppConfig {
    nick: String,
    provider: String,
    endpoint: String,
    timite_id: Option<u64>,
    workdir: Option<PathBuf>,
    timeout_secs: u64,
    max_output_chars: usize,
    claude_allowed_tools: Vec<String>,
}

fn load_config() -> Result<AppConfig, ConfigError> {
    Config::builder()
        .add_source(File::new(CONFIG_PATH, FileFormat::Toml))
        .build()?
        .try_deserialize()
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
        nick: config.nick,
        provider: config.provider,
        timite_id: config.timite_id,
    };

    let code_conf = CodeConf {
        workdir: config.workdir.map_or_else(std::env::current_dir, Ok)?,
        timeout: Duration::from_secs(config.timeout_secs),
        max_output_chars: config.max_output_chars,
        claude_allowed_tools: config.claude_allowed_tools,
    };

    agent::spawn(tim_conf, code_conf).await?;
    Ok(())
}
