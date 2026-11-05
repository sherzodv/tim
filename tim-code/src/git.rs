//! Git abilities over the workspace repository.

use crate::shell::Shell;
use crate::shell::ShellError;
use crate::text::head;

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error(transparent)]
    Shell(#[from] ShellError),

    #[error("git {command} failed: {stderr}")]
    Failed { command: String, stderr: String },
}

#[derive(Clone)]
pub struct Git {
    shell: Shell,
    max_output_chars: usize,
}

impl Git {
    pub fn new(shell: Shell, max_output_chars: usize) -> Self {
        Self {
            shell,
            max_output_chars,
        }
    }

    pub async fn status(&self) -> Result<String, GitError> {
        self.run(&["status", "--short", "--branch"]).await
    }

    pub async fn diff(&self, path: Option<&str>, staged: bool) -> Result<String, GitError> {
        let mut args = vec!["diff"];
        if staged {
            args.push("--staged");
        }
        if let Some(path) = path {
            args.extend(["--", path]);
        }
        self.run(&args).await
    }

    pub async fn log(&self, count: usize) -> Result<String, GitError> {
        let count = count.to_string();
        self.run(&["log", "--oneline", "-n", &count]).await
    }

    /// Stages the given paths, or everything, and commits.
    pub async fn commit(&self, message: &str, paths: &[String]) -> Result<String, GitError> {
        let mut add = vec!["add"];
        if paths.is_empty() {
            add.push("-A");
        } else {
            add.push("--");
            add.extend(paths.iter().map(String::as_str));
        }
        self.run(&add).await?;
        self.run(&["commit", "-q", "-m", message]).await?;
        self.run(&["log", "--oneline", "-n", "1"]).await
    }

    async fn run(&self, args: &[&str]) -> Result<String, GitError> {
        let output = self.shell.output("git", args).await?;
        if !output.status.success() {
            return Err(GitError::Failed {
                command: args.join(" "),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(head(stdout.trim_end(), self.max_output_chars))
    }
}
