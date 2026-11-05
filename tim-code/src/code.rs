use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use tim_client::agent::Agent;
use tim_client::agent::AgentBuilder;
use tim_client::agent::AgentError;
use tim_client::api::space_event::Data as Event;
use tim_client::api::Ability;
use tim_client::api::AbilityParameter;
use tim_client::api::CallAbility;
use tim_client::api::CallAbilityOutcome;
use tim_client::api::SpaceEvent;
use tim_client::client::TimClient;
use tracing::error;
use tracing::info;
use tracing::warn;

use crate::activity::Reporter;
use crate::claude::ClaudeCode;
use crate::claude::ClaudeError;
use crate::fs::Files;
use crate::fs::FsError;
use crate::git::Git;
use crate::git::GitError;
use crate::shell::Shell;
use crate::shell::ShellError;

const SH_RUN: &str = "sh.run";
const CODE_TASK: &str = "code.task";
const FS_READ: &str = "fs.read";
const FS_WRITE: &str = "fs.write";
const FS_REPLACE: &str = "fs.replace";
const FS_LIST: &str = "fs.list";
const FS_SEARCH: &str = "fs.search";
const GIT_STATUS: &str = "git.status";
const GIT_DIFF: &str = "git.diff";
const GIT_LOG: &str = "git.log";
const GIT_COMMIT: &str = "git.commit";

const DEFAULT_LOG_COUNT: usize = 10;

#[derive(Clone)]
pub struct CodeConf {
    pub workdir: PathBuf,
    pub timeout: Duration,
    pub max_output_chars: usize,
    pub claude_allowed_tools: Vec<String>,
}

/// Everything an ability call can reach; cloned into the task that serves the call.
#[derive(Clone)]
struct Tools {
    shell: Shell,
    claude: ClaudeCode,
    files: Files,
    git: Git,
}

pub struct CodeAgent {
    client: TimClient,
    tools: Tools,
}

#[derive(Debug, thiserror::Error)]
enum CallError {
    #[error("unknown ability: {0}")]
    UnknownAbility(String),

    #[error("invalid payload: {0}")]
    Payload(#[from] serde_json::Error),

    #[error(transparent)]
    Shell(#[from] ShellError),

    #[error(transparent)]
    Claude(#[from] ClaudeError),

    #[error(transparent)]
    Fs(#[from] FsError),

    #[error(transparent)]
    Git(#[from] GitError),
}

#[derive(Deserialize)]
struct ShRunReq {
    cmd: String,
}

#[derive(Deserialize)]
struct CodeTaskReq {
    description: String,
}

#[derive(Deserialize)]
struct FsReadReq {
    path: String,
    from: Option<usize>,
    to: Option<usize>,
}

#[derive(Deserialize)]
struct FsWriteReq {
    path: String,
    content: String,
}

#[derive(Deserialize)]
struct FsReplaceReq {
    path: String,
    old: String,
    new: String,
}

#[derive(Deserialize)]
struct FsListReq {
    #[serde(default)]
    path: String,
}

#[derive(Deserialize)]
struct FsSearchReq {
    pattern: String,
    #[serde(default)]
    path: String,
}

#[derive(Deserialize)]
struct GitDiffReq {
    path: Option<String>,
    #[serde(default)]
    staged: bool,
}

#[derive(Deserialize)]
struct GitLogReq {
    #[serde(default = "default_log_count")]
    count: usize,
}

#[derive(Deserialize)]
struct GitCommitReq {
    message: String,
    #[serde(default)]
    paths: Vec<String>,
}

fn default_log_count() -> usize {
    DEFAULT_LOG_COUNT
}

impl Tools {
    fn new(conf: &CodeConf) -> Self {
        let shell = Shell::new(conf.workdir.clone(), conf.timeout, conf.max_output_chars);
        Self {
            claude: ClaudeCode::new(shell.clone(), conf.claude_allowed_tools.clone()),
            files: Files::new(shell.clone(), conf.max_output_chars),
            git: Git::new(shell.clone(), conf.max_output_chars),
            shell,
        }
    }
}

impl CodeAgent {
    pub fn new(conf: &CodeConf, client: TimClient) -> Self {
        Self {
            client,
            tools: Tools::new(conf),
        }
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
        let mut client = self.client.clone();
        let ability = call.name.clone();
        let payload = call.payload.clone();
        tokio::spawn(async move {
            let reporter = Reporter::start(client.clone(), ability.clone());
            let result = dispatch(&tools, &reporter, &ability, &payload).await;
            drop(reporter);
            let outcome = to_outcome(call_id, result);
            if let Err(err) = client.send_call_ability_outcome(&outcome).await {
                error!(call_id, %err, "failed to send call ability outcome");
            }
        });
    }
}

async fn dispatch(
    tools: &Tools,
    reporter: &Reporter,
    ability: &str,
    payload: &str,
) -> Result<String, CallError> {
    match ability {
        SH_RUN => {
            let req: ShRunReq = serde_json::from_str(payload)?;
            Ok(tools.shell.run(&req.cmd).await?)
        }
        CODE_TASK => {
            let req: CodeTaskReq = serde_json::from_str(payload)?;
            Ok(tools.claude.task(&req.description, reporter).await?)
        }
        FS_READ => {
            let req: FsReadReq = serde_json::from_str(payload)?;
            Ok(tools.files.read(&req.path, req.from, req.to)?)
        }
        FS_WRITE => {
            let req: FsWriteReq = serde_json::from_str(payload)?;
            Ok(tools.files.write(&req.path, &req.content)?)
        }
        FS_REPLACE => {
            let req: FsReplaceReq = serde_json::from_str(payload)?;
            Ok(tools.files.replace(&req.path, &req.old, &req.new)?)
        }
        FS_LIST => {
            let req: FsListReq = serde_json::from_str(payload)?;
            Ok(tools.files.list(&req.path)?)
        }
        FS_SEARCH => {
            let req: FsSearchReq = serde_json::from_str(payload)?;
            Ok(tools.files.search(&req.pattern, &req.path).await?)
        }
        GIT_STATUS => Ok(tools.git.status().await?),
        GIT_DIFF => {
            let req: GitDiffReq = serde_json::from_str(payload)?;
            Ok(tools.git.diff(req.path.as_deref(), req.staged).await?)
        }
        GIT_LOG => {
            let req: GitLogReq = serde_json::from_str(payload)?;
            Ok(tools.git.log(req.count).await?)
        }
        GIT_COMMIT => {
            let req: GitCommitReq = serde_json::from_str(payload)?;
            Ok(tools.git.commit(&req.message, &req.paths).await?)
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
    vec![
        ability(
            SH_RUN,
            "Runs a shell command in the workspace directory, returns exit code and output tail. \
             Payload: JSON object.",
            &[("cmd", "shell command")],
        ),
        ability(
            CODE_TASK,
            "Delegates a coding task to Claude Code in the workspace: it reads, edits, builds \
             and tests as needed. Returns its summary and the git status. Payload: JSON object.",
            &[("description", "what to do, concrete and self-contained")],
        ),
        ability(
            FS_READ,
            "Reads a workspace file as numbered lines. Payload: JSON object.",
            &[
                ("path", "relative path"),
                ("from", "first line, 1-based, optional"),
                ("to", "last line, inclusive, optional"),
            ],
        ),
        ability(
            FS_WRITE,
            "Creates or overwrites a workspace file. Payload: JSON object.",
            &[("path", "relative path"), ("content", "full file content")],
        ),
        ability(
            FS_REPLACE,
            "Replaces one exact occurrence of old text in a workspace file; old must be unique. \
             Payload: JSON object.",
            &[
                ("path", "relative path"),
                ("old", "exact text to find"),
                ("new", "replacement text"),
            ],
        ),
        ability(
            FS_LIST,
            "Lists one directory of the workspace, directories end with a slash. \
             Payload: JSON object.",
            &[("path", "relative path, optional, workspace root by default")],
        ),
        ability(
            FS_SEARCH,
            "Searches workspace files by regex, returns path:line:text. Payload: JSON object.",
            &[
                ("pattern", "regex"),
                ("path", "relative path to search in, optional"),
            ],
        ),
        ability(GIT_STATUS, "Shows the branch and changed files.", &[]),
        ability(
            GIT_DIFF,
            "Shows the diff of the workspace. Payload: JSON object.",
            &[
                ("path", "relative path, optional"),
                ("staged", "true for the staged diff, optional"),
            ],
        ),
        ability(
            GIT_LOG,
            "Shows recent commits, one per line. Payload: JSON object.",
            &[("count", "number of commits, optional, 10 by default")],
        ),
        ability(
            GIT_COMMIT,
            "Stages the given paths, or all changes, and commits them. Payload: JSON object.",
            &[
                ("message", "commit message"),
                (
                    "paths",
                    "relative paths to stage, optional, everything by default",
                ),
            ],
        ),
    ]
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
            workdir = %self.tools.shell.workdir().display(),
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
