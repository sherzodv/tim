//! Delegates coding tasks to Claude Code running in the workspace.
//!
//! One `claude` process stays open and takes tasks as turns over stream-json stdin, so a
//! follow-up does not pay for startup and context loading again. After an idle period it
//! is closed; the next task starts a new process that resumes the same session.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use serde::Deserialize;
use serde_json::json;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::io::Lines;
use tokio::process::Child;
use tokio::process::ChildStdin;
use tokio::process::ChildStdout;
use tokio::sync::Mutex;
use tokio::time::sleep;
use tokio::time::timeout;
use tracing::info;

use crate::activity::Reporter;
use crate::shell::Shell;
use crate::shell::ShellError;
use crate::text::head;

const STEP_CHARS: usize = 80;
/// How long an idle `claude` process is kept before it is closed.
const IDLE_TIMEOUT: Duration = Duration::from_secs(600);

#[derive(Debug, thiserror::Error)]
pub enum ClaudeError {
    #[error(transparent)]
    Shell(#[from] ShellError),

    #[error("claude stream is not json: {0}")]
    Stream(#[from] serde_json::Error),

    #[error("claude stream ended without a result")]
    NoResult,

    #[error("task failed: {0}")]
    Task(String),
}

/// One line of `--output-format stream-json`.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum StreamEvent {
    Assistant {
        message: AssistantMessage,
    },
    Result(TaskResult),
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
struct AssistantMessage {
    #[serde(default)]
    content: Vec<ContentBlock>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ContentBlock {
    Text {
        text: String,
    },
    ToolUse {
        name: String,
        #[serde(default)]
        input: serde_json::Value,
    },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
struct TaskResult {
    #[serde(default)]
    is_error: bool,
    #[serde(default)]
    result: String,
    #[serde(default)]
    num_turns: u32,
    total_cost_usd: Option<f64>,
    session_id: Option<String>,
}

/// A running `claude` process taking turns over stdin.
struct Live {
    _child: Child,
    stdin: ChildStdin,
    events: Lines<BufReader<ChildStdout>>,
    last_used: Instant,
}

/// The Claude Code session of the workspace; tasks run one at a time.
#[derive(Default)]
struct Session {
    live: Option<Live>,
    id: Option<String>,
}

#[derive(Clone)]
pub struct ClaudeCode {
    shell: Shell,
    /// Tools Claude may use without asking; file edits are always accepted.
    allowed_tools: Vec<String>,
    session: Arc<Mutex<Session>>,
}

impl ClaudeCode {
    pub fn new(shell: Shell, allowed_tools: Vec<String>) -> Self {
        let session = Arc::new(Mutex::new(Session::default()));
        tokio::spawn(close_when_idle(session.clone()));
        Self {
            shell,
            allowed_tools,
            session,
        }
    }

    /// Runs the task as the next turn, reporting each step, and returns Claude's summary.
    pub async fn task(
        &self,
        description: &str,
        reporter: &Reporter,
    ) -> Result<String, ClaudeError> {
        let mut session = self.session.lock().await;
        if session.live.is_none() {
            session.live = Some(self.open(session.id.as_deref()).await?);
        }
        let live = session.live.as_mut().expect("opened above");

        let turn = json!({
            "type": "user",
            "message": { "role": "user", "content": [{ "type": "text", "text": description }] },
        });
        live.stdin
            .write_all(format!("{turn}\n").as_bytes())
            .await
            .map_err(ShellError::from)?;

        let followed = timeout(
            self.shell.timeout(),
            self.follow(&mut live.events, reporter),
        )
        .await;
        let result = match followed {
            Ok(Ok(result)) => result,
            Ok(Err(error)) => {
                session.live = None;
                return Err(error);
            }
            Err(_) => {
                session.live = None;
                return Err(ShellError::Timeout(self.shell.timeout()).into());
            }
        };
        live.last_used = Instant::now();
        if result.session_id.is_some() {
            session.id = result.session_id;
        }

        info!(
            turns = result.num_turns,
            cost_usd = result.total_cost_usd,
            session = session.id.as_deref().unwrap_or(""),
            "claude task finished"
        );
        if result.is_error {
            return Err(ClaudeError::Task(result.result));
        }
        Ok(result.result.trim().to_string())
    }

    /// Starts a `claude` process, resuming `session_id` when there is one.
    async fn open(&self, session_id: Option<&str>) -> Result<Live, ClaudeError> {
        let mut args = vec![
            "-p",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
            "--permission-mode",
            "acceptEdits",
        ];
        if let Some(id) = session_id {
            args.extend(["--resume", id]);
        }
        if !self.allowed_tools.is_empty() {
            args.push("--allowedTools");
            args.extend(self.allowed_tools.iter().map(String::as_str));
        }
        let mut child = self.shell.spawn("claude", &args).await?;
        let stdin = child.stdin.take().expect("stdin is piped");
        let stdout = child.stdout.take().expect("stdout is piped");
        info!(resumed = session_id.is_some(), "claude process started");
        Ok(Live {
            _child: child,
            stdin,
            events: BufReader::new(stdout).lines(),
            last_used: Instant::now(),
        })
    }

    /// Reads the turn's events, turning tool uses and narration into activity steps.
    async fn follow(
        &self,
        events: &mut Lines<BufReader<ChildStdout>>,
        reporter: &Reporter,
    ) -> Result<TaskResult, ClaudeError> {
        while let Some(line) = events.next_line().await.map_err(ShellError::from)? {
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<StreamEvent>(&line)? {
                StreamEvent::Assistant { message } => {
                    for block in message.content {
                        if let Some(step) = self.describe(block) {
                            reporter.step(step);
                        }
                    }
                }
                StreamEvent::Result(result) => return Ok(result),
                StreamEvent::Other => {}
            }
        }
        Err(ClaudeError::NoResult)
    }

    fn describe(&self, block: ContentBlock) -> Option<String> {
        let step = match block {
            ContentBlock::Text { text } => text.lines().next()?.trim().to_string(),
            ContentBlock::ToolUse { name, input } => match tool_detail(&name, &input) {
                Some(detail) => format!("{name} {}", self.relative(&detail)),
                None => name,
            },
            ContentBlock::Other => return None,
        };
        Some(head(&step, STEP_CHARS))
    }

    fn relative(&self, path: &str) -> String {
        Path::new(path)
            .strip_prefix(self.shell.workdir())
            .map_or_else(|_| path.to_string(), |rel| rel.display().to_string())
    }
}

/// Closes the process once no task has used it for `IDLE_TIMEOUT`.
async fn close_when_idle(session: Arc<Mutex<Session>>) {
    loop {
        sleep(IDLE_TIMEOUT / 2).await;
        let mut session = session.lock().await;
        let idle = session
            .live
            .as_ref()
            .is_some_and(|live| live.last_used.elapsed() >= IDLE_TIMEOUT);
        if idle {
            info!("closing idle claude process");
            session.live = None;
        }
    }
}

/// The argument that tells a tool call apart, in the words Claude Code uses.
fn tool_detail(name: &str, input: &serde_json::Value) -> Option<String> {
    let field = match name {
        "Bash" => input["description"].as_str().or(input["command"].as_str()),
        "Read" | "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => input["file_path"].as_str(),
        "Grep" | "Glob" => input["pattern"].as_str(),
        "WebFetch" => input["url"].as_str(),
        "WebSearch" => input["query"].as_str(),
        "Agent" | "Task" => input["description"].as_str(),
        _ => None,
    };
    field.map(str::to_string)
}
