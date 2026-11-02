use std::path::Path;
use std::path::PathBuf;
use std::process::Output;
use std::process::Stdio;
use std::time::Duration;

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

    pub async fn run(&self, cmd: &str) -> Result<String, ShellError> {
        let output = Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .current_dir(&self.workdir)
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output();
        let output = timeout(self.timeout, output)
            .await
            .map_err(|_| ShellError::Timeout(self.timeout))??;
        Ok(self.render_output(&output))
    }

    fn render_output(&self, output: &Output) -> String {
        let exit_code = output
            .status
            .code()
            .map_or_else(|| "signal".to_string(), |code| code.to_string());
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        format!(
            "exit_code: {exit_code}\nstdout:\n{}\nstderr:\n{}",
            tail(stdout.trim_end(), self.max_output_chars),
            tail(stderr.trim_end(), self.max_output_chars),
        )
    }
}

/// Keeps the last `max_chars` characters: errors and summaries are usually at the end.
fn tail(text: &str, max_chars: usize) -> String {
    let len = text.chars().count();
    if len <= max_chars {
        return text.to_string();
    }
    let tail: String = text.chars().skip(len - max_chars).collect();
    format!("…{tail}")
}
