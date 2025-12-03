mod agent;
mod crawler;
mod llm;
mod tim_client;

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
use toml_edit::value;
use toml_edit::DocumentMut;
use tracing::warn;
use tracing_subscriber::fmt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

use crate::crawler::CrawlerConf;
use crate::llm::AgentConf;
use crate::llm::OPENAI_DEFAULT_ENDPOINT;
use crate::tim_client::TimClient;
use crate::tim_client::TimClientConf;

const CONFIG_PATH: &str = "agents.toml";

struct LoadedConfig {
    config: AppConfig,
    doc: DocumentMut,
}

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
    nick: String,
    provider: String,
    endpoint: String,
    prompt: String,
    model: String,
    temperature: f32,
    live_interval_secs: Option<u64>,
    api_key: String,
    timite_id: Option<u64>,
}

#[derive(Deserialize)]
struct CrawlerAgentConfig {
    nick: String,
    provider: String,
    endpoint: String,
    ability_name: String,
    max_snippet_chars: usize,
    user_agent: String,
    timite_id: Option<u64>,
}

fn load_prompt(prompts_dir: &Path, name: &str) -> Result<String, Box<dyn std::error::Error>> {
    let prompt_path = prompts_dir.join(name);
    Ok(fs::read_to_string(prompt_path)?)
}

fn load_config() -> Result<LoadedConfig, Box<dyn std::error::Error>> {
    dotenv().ok();

    let raw = fs::read_to_string(CONFIG_PATH)?;
    let doc: DocumentMut = raw.parse()?;
    let expanded = expand_env(&raw)?.into_owned();

    let config = Config::builder()
        .add_source(File::from_str(&expanded, FileFormat::Toml))
        .add_source(Environment::with_prefix("TIM_AGENT").separator("__"))
        .build()?
        .try_deserialize()?;

    Ok(LoadedConfig { config, doc })
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

fn spawn_llm_agent(
    conf: LlmAgentConfig,
    prompts_dir: &Path,
) -> Result<BoxFuture<'static, Result<(), agent::AgentError>>, Box<dyn std::error::Error>> {
    let sysp = load_prompt(prompts_dir, &conf.prompt)?;

    let tim_conf = TimClientConf {
        nick: conf.nick,
        provider: conf.provider,
        endpoint: conf.endpoint,
        timite_id: conf.timite_id,
    };

    let llm_conf = AgentConf {
        sysp,
        api_key: conf.api_key,
        endpoint: OPENAI_DEFAULT_ENDPOINT.to_string(),
        model: conf.model,
        temperature: conf.temperature,
        live_interval: conf.live_interval_secs.map(Duration::from_secs),
    };

    Ok(Box::pin(
        async move { agent::spawn(tim_conf, llm_conf).await },
    ))
}

fn spawn_crawler_agent(
    conf: CrawlerAgentConfig,
) -> Result<BoxFuture<'static, Result<(), agent::AgentError>>, Box<dyn std::error::Error>> {
    let tim_conf = TimClientConf {
        nick: conf.nick,
        provider: conf.provider,
        endpoint: conf.endpoint,
        timite_id: conf.timite_id,
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

fn update_timite_in_doc(
    doc: &mut DocumentMut,
    index: usize,
    timite_id: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let agents = doc
        .get_mut("agents")
        .and_then(|item| item.as_array_of_tables_mut())
        .ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "agents config missing")
        })?;
    let table = agents.get_mut(index).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "agents config index missing",
        )
    })?;
    table["timite_id"] = value(timite_id as i64);
    Ok(())
}

async fn register_timite(
    endpoint: &str,
    nick: &str,
    provider: &str,
) -> Result<u64, Box<dyn std::error::Error>> {
    let client = TimClient::new(TimClientConf {
        endpoint: endpoint.to_string(),
        nick: nick.to_string(),
        provider: provider.to_string(),
        timite_id: None,
    })
    .await?;
    Ok(client.timite_id())
}

async fn ensure_timite_ids(loaded: &mut LoadedConfig) -> Result<(), Box<dyn std::error::Error>> {
    let mut updated = false;

    for (index, agent) in loaded.config.agents.iter_mut().enumerate() {
        let (timite_slot, endpoint, nick, provider) = match agent {
            AgentConfig::Llm(conf) => (
                &mut conf.timite_id,
                conf.endpoint.as_str(),
                conf.nick.as_str(),
                conf.provider.as_str(),
            ),
            AgentConfig::Crawler(conf) => (
                &mut conf.timite_id,
                conf.endpoint.as_str(),
                conf.nick.as_str(),
                conf.provider.as_str(),
            ),
        };

        if let Some(timite_id) = timite_slot {
            let probe_conf = TimClientConf {
                endpoint: endpoint.to_string(),
                nick: nick.to_string(),
                provider: provider.to_string(),
                timite_id: Some(*timite_id),
            };
            if TimClient::new(probe_conf.clone()).await.is_ok() {
                continue;
            }
            warn!(
                timite_id,
                "timite not found when connecting, re-registering"
            );
        }

        let timite_id = register_timite(endpoint, nick, provider).await?;
        *timite_slot = Some(timite_id);
        update_timite_in_doc(&mut loaded.doc, index, timite_id)?;
        updated = true;
    }

    if updated {
        fs::write(CONFIG_PATH, loaded.doc.to_string())?;
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env())
        .with(fmt::layer())
        .init();

    let mut loaded_config = load_config()?;
    ensure_timite_ids(&mut loaded_config).await?;

    let prompts_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("prompts");

    let agents = loaded_config
        .config
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
