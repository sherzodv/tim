use std::path::Path;
use std::path::PathBuf;
use std::process::Output;
use std::process::Stdio;
use std::time::Duration;

use tokio::process::Child;
use tokio::process::Command;
use tokio::time::timeout;

#[derive(Debug, thiserror::Error)]
pub enum ShellError {
    #[error("failed to run shell: {0}")]
    Io(#[from] std::io::Error),

    #[error("command timed out after {0:?}")]
    Timeout(Duration),
}

/// Runs shell commands locally in the workspace directory.
#[derive(Clone)]
pub struct Shell {
    workdir: PathBuf,
    timeout: Duration,
    max_output_chars: usize,
}

impl Shell {
    pub fn new(workdir: PathBuf, timeout: Duration, max_output_chars: usize) -> Self {
        Self {
            workdir,
            timeout,
            max_output_chars,
        }
    }

    pub fn workdir(&self) -> &Path {
        &self.workdir
    }

    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    pub fn max_output_chars(&self) -> usize {
        self.max_output_chars
    }

    /// Runs a program in the workspace directory and returns its raw output.
    pub async fn output(&self, program: &str, args: &[&str]) -> Result<Output, ShellError> {
        let output = Command::new(program)
            .args(args)
            .current_dir(&self.workdir)
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output();
        timeout(self.timeout, output)
            .await
            .map_err(|_| ShellError::Timeout(self.timeout))?
            .map_err(Into::into)
    }

    /// Starts a program in the workspace directory with stdin and stdout piped; the
    /// caller owns the timeout and the child is killed when dropped.
    pub async fn spawn(&self, program: &str, args: &[&str]) -> Result<Child, ShellError> {
        Ok(Command::new(program)
            .args(args)
            .current_dir(&self.workdir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?)
    }
}
