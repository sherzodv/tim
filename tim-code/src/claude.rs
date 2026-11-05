//! Delegates coding tasks to Claude Code running non-interactively in the workspace.

use std::path::Path;

use serde::Deserialize;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::process::ChildStdout;
use tokio::time::timeout;
use tracing::info;

use crate::activity::Reporter;
use crate::shell::exit_code;
use crate::shell::Shell;
use crate::shell::ShellError;
use crate::text::head;

const STEP_CHARS: usize = 80;

#[derive(Debug, thiserror::Error)]
pub enum ClaudeError {
    #[error(transparent)]
    Shell(#[from] ShellError),

    #[error("claude exited with {0}")]
    Exit(String),

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
}

#[derive(Clone)]
pub struct ClaudeCode {
    shell: Shell,
    /// Tools Claude may use without asking; file edits are always accepted.
    allowed_tools: Vec<String>,
}

impl ClaudeCode {
    pub fn new(shell: Shell, allowed_tools: Vec<String>) -> Self {
        Self {
            shell,
            allowed_tools,
        }
    }

    /// Runs the task to completion, reporting each step, and returns Claude's summary
    /// with the resulting git status.
    pub async fn task(
        &self,
        description: &str,
        reporter: &Reporter,
    ) -> Result<String, ClaudeError> {
        let mut args = vec![
            "-p",
            description,
            "--output-format",
            "stream-json",
            "--verbose",
            "--permission-mode",
            "acceptEdits",
        ];
        if !self.allowed_tools.is_empty() {
            args.push("--allowedTools");
            args.extend(self.allowed_tools.iter().map(String::as_str));
        }

        let mut child = self.shell.spawn("claude", &args).await?;
        let stdout = child.stdout.take().expect("stdout is piped");
        let result = timeout(self.shell.timeout(), self.follow(stdout, reporter))
            .await
            .map_err(|_| ShellError::Timeout(self.shell.timeout()))??;
        let status = child.wait().await.map_err(ShellError::from)?;
        if !status.success() {
            return Err(ClaudeError::Exit(exit_code(status)));
        }

        info!(
            turns = result.num_turns,
            cost_usd = result.total_cost_usd,
            "claude task finished"
        );
        if result.is_error {
            return Err(ClaudeError::Task(result.result));
        }

        let git_status = self.shell.output("git", &["status", "--short"]).await?;
        let git_status = String::from_utf8_lossy(&git_status.stdout);
        Ok(format!(
            "summary:\n{}\n\ngit status:\n{}",
            result.result.trim(),
            git_status.trim()
        ))
    }

    /// Reads the event stream, turning tool uses and narration into activity steps.
    async fn follow(
        &self,
        stdout: ChildStdout,
        reporter: &Reporter,
    ) -> Result<TaskResult, ClaudeError> {
        let mut lines = BufReader::new(stdout).lines();
        let mut result = None;
        while let Some(line) = lines.next_line().await.map_err(ShellError::from)? {
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
                StreamEvent::Result(task_result) => result = Some(task_result),
                StreamEvent::Other => {}
            }
        }
        result.ok_or(ClaudeError::NoResult)
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
