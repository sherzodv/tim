//! Delegates coding tasks to Claude Code running non-interactively in the workspace.

use serde::Deserialize;
use tracing::info;

use crate::shell::exit_code;
use crate::shell::Shell;
use crate::shell::ShellError;
use crate::text::tail;

const STDERR_TAIL_CHARS: usize = 2000;

#[derive(Debug, thiserror::Error)]
pub enum ClaudeError {
    #[error(transparent)]
    Shell(#[from] ShellError),

    #[error("claude exited with {code}: {stderr}")]
    Exit { code: String, stderr: String },

    #[error("claude result is not json: {0}")]
    Result(#[from] serde_json::Error),

    #[error("task failed: {0}")]
    Task(String),
}

/// The `--output-format json` envelope of `claude -p`.
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

    /// Runs the task to completion and reports Claude's summary with the resulting git status.
    pub async fn task(&self, description: &str) -> Result<String, ClaudeError> {
        let mut args = vec![
            "-p",
            description,
            "--output-format",
            "json",
            "--permission-mode",
            "acceptEdits",
            "--allowedTools",
        ];
        args.extend(self.allowed_tools.iter().map(String::as_str));

        let output = self.shell.output("claude", &args).await?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ClaudeError::Exit {
                code: exit_code(&output),
                stderr: tail(stderr.trim_end(), STDERR_TAIL_CHARS),
            });
        }

        let result: TaskResult = serde_json::from_slice(&output.stdout)?;
        info!(
            turns = result.num_turns,
            cost_usd = result.total_cost_usd,
            "claude task finished"
        );
        if result.is_error {
            return Err(ClaudeError::Task(result.result));
        }

        let status = self.shell.output("git", &["status", "--short"]).await?;
        let status = String::from_utf8_lossy(&status.stdout);
        Ok(format!(
            "summary:\n{}\n\ngit status:\n{}",
            result.result.trim(),
            status.trim()
        ))
    }
}
