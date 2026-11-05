use async_trait::async_trait;
use tim_client::agent::Agent;
use tim_client::agent::AgentBuilder;
use tim_client::agent::AgentError;
use tim_client::api::space_event::Data as Event;
use tim_client::api::Ability;
use tim_client::api::AbilityParameter;
use tim_client::api::CallAbility;
use tim_client::api::CallAbilityOutcome;
use tim_client::api::Message;
use tim_client::api::SpaceEvent;
use tim_client::client::TimClient;
use tim_llm::llm::LlmConf;
use tracing::error;
use tracing::info;
use tracing::warn;

use crate::claude::ClaudeCode;
use crate::claude::ClaudeError;
use crate::git::GitError;
use crate::llm::LlmCode;
use crate::llm::LlmCodeError;
use crate::progress::Reporter;
use crate::text::head;
use crate::tools;
use crate::tools::ToolError;
use crate::tools::Tools;

const CODE_TASK: &str = "code.task";
/// How much of a chat request names the task it starts.
const TASK_CHARS: usize = 80;

#[derive(Clone)]
pub struct CodeConf {
    pub tools: Tools,
    pub engine: EngineConf,
}

/// What runs `code.task` and chat tasks.
#[derive(Clone)]
pub enum EngineConf {
    Claude {
        allowed_tools: Vec<String>,
    },
    Llm {
        provider: String,
        conf: LlmConf,
        max_steps: usize,
    },
}

#[derive(Clone)]
enum Engine {
    Claude(ClaudeCode),
    Llm(LlmCode),
}

pub struct CodeAgent {
    client: TimClient,
    tools: Tools,
    engine: Engine,
}

#[derive(Debug, thiserror::Error)]
enum CallError {
    #[error(transparent)]
    Tool(#[from] ToolError),

    #[error(transparent)]
    Claude(#[from] ClaudeError),

    #[error(transparent)]
    Llm(#[from] LlmCodeError),

    #[error(transparent)]
    Git(#[from] GitError),
}

impl Engine {
    fn new(conf: &EngineConf, tools: &Tools) -> Result<Self, AgentError> {
        match conf {
            EngineConf::Claude { allowed_tools } => Ok(Self::Claude(ClaudeCode::new(
                tools.shell.clone(),
                allowed_tools.clone(),
            ))),
            EngineConf::Llm {
                provider,
                conf,
                max_steps,
            } => LlmCode::new(provider, conf, tools.clone(), *max_steps)
                .map(Self::Llm)
                .map_err(|err| AgentError::Agent(err.to_string())),
        }
    }

    async fn task(&self, description: &str, reporter: &Reporter) -> Result<String, CallError> {
        match self {
            Self::Claude(claude) => Ok(claude.task(description, reporter).await?),
            Self::Llm(llm) => Ok(llm.task(description, reporter).await?),
        }
    }
}

impl CodeAgent {
    pub fn new(conf: &CodeConf, client: TimClient) -> Result<Self, AgentError> {
        Ok(Self {
            client,
            tools: conf.tools.clone(),
            engine: Engine::new(&conf.engine, &conf.tools)?,
        })
    }

    /// Runs a message that mentions this timite as a task and replies with the report.
    fn handle_message(&self, message: &Message) {
        let me = self.client.get_me();
        if !message
            .mentions
            .iter()
            .any(|mention| mention.timite_id == me.id)
        {
            return;
        }
        let task = without_mention(&message.content, &me.username);
        if task.is_empty() {
            return;
        }
        let tools = self.tools.clone();
        let engine = self.engine.clone();
        let mut client = self.client.clone();
        tokio::spawn(async move {
            let reporter = Reporter::start(client.clone(), head(&task, TASK_CHARS));
            let result = report(&tools, &engine, &task, &reporter).await;
            reporter.finish().await;
            let reply = match result {
                Ok(report) => report,
                Err(err) => format!("task failed: {err}"),
            };
            if let Err(err) = client.send_message(&reply).await {
                error!(%err, "failed to send task reply");
            }
        });
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
        let tools = self.tools.clone();
        let engine = self.engine.clone();
        let mut client = self.client.clone();
        let ability = call.name.clone();
        let payload = call.payload.clone();
        tokio::spawn(async move {
            let reporter = Reporter::start(client.clone(), ability.clone());
            let result = dispatch(&tools, &engine, &reporter, &ability, &payload).await;
            reporter.finish().await;
            let outcome = to_outcome(call_id, result);
            if let Err(err) = client.send_call_ability_outcome(&outcome).await {
                error!(call_id, %err, "failed to send call ability outcome");
            }
        });
    }
}

async fn dispatch(
    tools: &Tools,
    engine: &Engine,
    reporter: &Reporter,
    ability: &str,
    payload: &str,
) -> Result<String, CallError> {
    if ability != CODE_TASK {
        return Ok(tools::call(tools, ability, payload).await?);
    }
    let description = serde_json::from_str::<serde_json::Value>(payload)
        .map_err(ToolError::Arguments)?["description"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    report(tools, engine, &description, reporter).await
}

/// The engine's summary of the task followed by the resulting git status.
async fn report(
    tools: &Tools,
    engine: &Engine,
    description: &str,
    reporter: &Reporter,
) -> Result<String, CallError> {
    let summary = engine.task(description, reporter).await?;
    let git_status = tools.git.status().await?;
    Ok(format!("summary:\n{summary}\n\ngit status:\n{git_status}"))
}

/// The message without its `@username` tokens, trimmed.
fn without_mention(content: &str, username: &str) -> String {
    let token = format!("@{username}");
    let mut text = String::with_capacity(content.len());
    let mut rest = content;
    while let Some(at) = rest.find(&token) {
        let after = &rest[at + token.len()..];
        let whole_token =
            !after.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        text.push_str(&rest[..at]);
        if !whole_token {
            text.push_str(&token);
        }
        rest = after;
    }
    text.push_str(rest);
    text.trim().to_string()
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
    let mut abilities = tools::abilities();
    abilities.push(Ability {
        name: CODE_TASK.to_string(),
        description: "Runs a coding task in the workspace: reads, edits, builds and tests as \
                      needed. Returns a summary and the git status. Payload: JSON object."
            .to_string(),
        params: vec![AbilityParameter {
            name: "description".to_string(),
            description: "what to do, concrete and self-contained".to_string(),
        }],
    });
    abilities
}

#[async_trait]
impl Agent for CodeAgent {
    async fn on_start(&mut self) -> Result<(), AgentError> {
        self.declare().await?;
        info!(
            timite_id = self.client.timite_id(),
            workdir = %self.tools.shell.workdir().display(),
            "code abilities declared"
        );
        Ok(())
    }

    async fn on_space_update(&mut self, update: &SpaceEvent) -> Result<(), AgentError> {
        match &update.data {
            Some(Event::EventCallAbility(event)) => {
                if let Some(call) = &event.call_ability {
                    self.handle_call(call);
                }
            }
            Some(Event::EventNewMessage(event)) => {
                if let Some(message) = &event.message {
                    self.handle_message(message);
                }
            }
            _ => {}
        }
        Ok(())
    }
}

impl AgentBuilder for CodeConf {
    type A = CodeAgent;

    fn build(&self, client: TimClient) -> Result<Self::A, AgentError> {
        CodeAgent::new(self, client)
    }
}
