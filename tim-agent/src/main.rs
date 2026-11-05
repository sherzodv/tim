mod crawler;
mod llm;

use std::fs;
use std::path::Path;
use std::time::Duration;

use config::Config;
use config::Environment;
use config::File;
use config::FileFormat;
use dotenvy::dotenv;
use futures::future::try_join_all;
use futures::future::BoxFuture;
use serde::Deserialize;
use shellexpand::env as expand_env;
use tracing_subscriber::fmt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

use crate::crawler::CrawlerConf;
use crate::llm::AgentConf;
use tim_client::agent;
use tim_client::client::TimClientConf;
use tim_llm::llm::LlmConf;

const CONFIG_PATH: &str = "agents.toml";

#[derive(Deserialize)]
struct AppConfig {
    agents: Vec<AgentConfig>,
}

#[derive(Deserialize)]
#[serde(tag = "kind")]
enum AgentConfig {
    #[serde(rename = "llm")]
    Llm(LlmAgentConfig),
    #[serde(rename = "crawler")]
    Crawler(CrawlerAgentConfig),
}

#[derive(Deserialize)]
struct LlmAgentConfig {
    username: String,
    provider: String,
    endpoint: String,
    prompt: String,
    llm_endpoint: Option<String>,
    model: String,
    temperature: f32,
    live_interval_secs: Option<u64>,
    api_key: String,
}

#[derive(Deserialize)]
struct CrawlerAgentConfig {
    username: String,
    provider: String,
    endpoint: String,
    ability_name: String,
    max_snippet_chars: usize,
    user_agent: String,
}

fn load_prompt(prompts_dir: &Path, name: &str) -> Result<String, Box<dyn std::error::Error>> {
    let prompt_path = prompts_dir.join(name);
    Ok(fs::read_to_string(prompt_path)?)
}

fn load_config() -> Result<AppConfig, Box<dyn std::error::Error>> {
    dotenv().ok();

    let raw = fs::read_to_string(CONFIG_PATH)?;
    let expanded = expand_env(&raw)?.into_owned();

    let config = Config::builder()
        .add_source(File::from_str(&expanded, FileFormat::Toml))
        .add_source(Environment::with_prefix("TIM_AGENT").separator("__"))
        .build()?
        .try_deserialize()?;

    Ok(config)
}

fn spawn_agent(
    config: AgentConfig,
    prompts_dir: &Path,
) -> Result<BoxFuture<'static, Result<(), agent::AgentError>>, Box<dyn std::error::Error>> {
    match config {
        AgentConfig::Llm(conf) => spawn_llm_agent(conf, prompts_dir),
        AgentConfig::Crawler(conf) => spawn_crawler_agent(conf),
    }
}

/// The llm implementation named by a provider such as `openai:jarvis`.
fn llm_provider(provider: &str) -> &str {
    provider.split(':').next().unwrap_or(provider)
}

fn spawn_llm_agent(
    conf: LlmAgentConfig,
    prompts_dir: &Path,
) -> Result<BoxFuture<'static, Result<(), agent::AgentError>>, Box<dyn std::error::Error>> {
    let sysp = load_prompt(prompts_dir, &conf.prompt)?;

    let llm_conf = AgentConf {
        sysp,
        provider: llm_provider(&conf.provider).to_string(),
        llm: LlmConf {
            api_key: conf.api_key,
            endpoint: conf.llm_endpoint,
            model: conf.model,
            temperature: conf.temperature,
        },
        live_interval: conf.live_interval_secs.map(Duration::from_secs),
    };

    let tim_conf = TimClientConf {
        endpoint: conf.endpoint,
        username: conf.username,
        provider: conf.provider,
    };

    Ok(Box::pin(
        async move { agent::spawn(tim_conf, llm_conf).await },
    ))
}

fn spawn_crawler_agent(
    conf: CrawlerAgentConfig,
) -> Result<BoxFuture<'static, Result<(), agent::AgentError>>, Box<dyn std::error::Error>> {
    let tim_conf = TimClientConf {
        endpoint: conf.endpoint,
        username: conf.username,
        provider: conf.provider,
    };

    let crawler_conf = CrawlerConf {
        ability_name: conf.ability_name,
        max_snippet_chars: conf.max_snippet_chars,
        user_agent: conf.user_agent,
    };

    Ok(Box::pin(async move {
        agent::spawn(tim_conf, crawler_conf).await
    }))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env())
        .with(fmt::layer())
        .init();

    let config = load_config()?;

    let prompts_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("prompts");

    let agents = config
        .agents
        .into_iter()
        .map(|agent| spawn_agent(agent, &prompts_dir))
        .collect::<Result<Vec<_>, _>>()?;

    try_join_all(agents)
        .await
        .map(|_| ())
        .map_err(|err| Box::new(err) as Box<dyn std::error::Error>)?;

    Ok(())
}
