use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use tim_agent::agent::Agent;
use tim_agent::agent::AgentBuilder;
use tim_agent::agent::AgentError;
use tim_agent::tim_client::tim_api::Ability;
use tim_agent::tim_client::tim_api::AbilityParameter;
use tim_agent::tim_client::tim_api::CallAbility;
use tim_agent::tim_client::tim_api::CallAbilityOutcome;
use tim_agent::tim_client::Event;
use tim_agent::tim_client::SpaceEvent;
use tim_agent::tim_client::TimClient;
use tracing::error;
use tracing::info;
use tracing::warn;

use crate::shell::Shell;
use crate::shell::ShellError;

const SH_RUN: &str = "sh.run";

#[derive(Clone)]
pub struct CodeConf {
    pub workdir: PathBuf,
    pub timeout: Duration,
    pub max_output_chars: usize,
}

pub struct CodeAgent {
    client: TimClient,
    shell: Shell,
}

#[derive(Debug, thiserror::Error)]
enum CallError {
    #[error("unknown ability: {0}")]
    UnknownAbility(String),

    #[error("invalid payload: {0}")]
    Payload(#[from] serde_json::Error),

    #[error(transparent)]
    Shell(#[from] ShellError),
}

#[derive(Deserialize)]
struct ShRunReq {
    cmd: String,
}

impl CodeAgent {
    pub fn new(conf: &CodeConf, client: TimClient) -> Self {
        let shell = Shell::new(conf.workdir.clone(), conf.timeout, conf.max_output_chars);
        Self { client, shell }
    }

    async fn declare(&mut self) -> Result<(), AgentError> {
        self.client.declare_abilities(abilities()).await?;
        Ok(())
    }

    /// Runs the call in a separate task: commands can take minutes.
    fn handle_call(&self, call: &CallAbility) {
        if call.timite_id != self.client.timite_id() {
            return;
        }
        let Some(call_id) = call.call_ability_id else {
            warn!(ability = %call.name, "call ability without id");
            return;
        };
        let shell = self.shell.clone();
        let mut client = self.client.clone();
        let ability = call.name.clone();
        let payload = call.payload.clone();
        tokio::spawn(async move {
            let result = dispatch(&shell, &ability, &payload).await;
            let outcome = to_outcome(call_id, result);
            if let Err(err) = client.send_call_ability_outcome(&outcome).await {
                error!(call_id, %err, "failed to send call ability outcome");
            }
        });
    }
}

async fn dispatch(shell: &Shell, ability: &str, payload: &str) -> Result<String, CallError> {
    match ability {
        SH_RUN => {
            let req: ShRunReq = serde_json::from_str(payload)?;
            Ok(shell.run(&req.cmd).await?)
        }
        _ => Err(CallError::UnknownAbility(ability.to_string())),
    }
}

fn to_outcome(call_ability_id: u64, result: Result<String, CallError>) -> CallAbilityOutcome {
    match result {
        Ok(payload) => CallAbilityOutcome {
            call_ability_id,
            payload: Some(payload),
            error: None,
        },
        Err(err) => CallAbilityOutcome {
            call_ability_id,
            payload: None,
            error: Some(err.to_string()),
        },
    }
}

fn abilities() -> Vec<Ability> {
    vec![ability(
        SH_RUN,
        "Runs a shell command in the workspace directory, returns exit code and output tail. \
         Payload: JSON object.",
        &[("cmd", "shell command")],
    )]
}

fn ability(name: &str, description: &str, params: &[(&str, &str)]) -> Ability {
    Ability {
        name: name.to_string(),
        description: description.to_string(),
        params: params
            .iter()
            .map(|(name, description)| AbilityParameter {
                name: name.to_string(),
                description: description.to_string(),
            })
            .collect(),
    }
}

#[async_trait]
impl Agent for CodeAgent {
    async fn on_start(&mut self) -> Result<(), AgentError> {
        self.declare().await?;
        info!(
            timite_id = self.client.timite_id(),
            workdir = %self.shell.workdir().display(),
            "code abilities declared"
        );
        Ok(())
    }

    async fn on_space_update(&mut self, update: &SpaceEvent) -> Result<(), AgentError> {
        if let Some(Event::EventCallAbility(event)) = &update.data {
            if let Some(call) = &event.call_ability {
                self.handle_call(call);
            }
        }
        Ok(())
    }
}

impl AgentBuilder for CodeConf {
    type A = CodeAgent;

    fn build(&self, client: TimClient) -> Result<Self::A, AgentError> {
        Ok(CodeAgent::new(self, client))
    }
}
