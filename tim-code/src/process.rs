//! Task-owned shell processes. The space is the output log; only tails stay in memory.
//! Unix process groups delimit ownership: detached/daemon processes are not supported.

use std::collections::HashMap;
use std::io;
use std::os::unix::process::ExitStatusExt;
use std::process::{ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde_json::json;
use tim_client::api::process_finished::Reason;
use tim_client::api::process_output::Stream;
use tim_client::api::task_process_event::Data;
use tim_client::api::{ProcessFinished, ProcessOutput, ProcessStarted};
use tokio::io::AsyncReadExt;
use tokio::process::{Child, ChildStderr, ChildStdout, Command};
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::progress::{ReportError, Reporter};
use crate::shell::Shell;
use crate::text::tail;

const CHUNK_BYTES: usize = 8192;

#[derive(Debug, thiserror::Error)]
pub enum ProcessError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Report(#[from] ReportError),
    #[error("process is not running in this branch: {0}")]
    NotRunning(String),
    #[error("command is empty")]
    EmptyCommand,
}

struct Control {
    branch_id: u64,
    stop: Option<oneshot::Sender<()>>,
}

#[derive(Clone, Default)]
pub struct Processes {
    running: Arc<Mutex<HashMap<String, Control>>>,
}

impl Processes {
    pub async fn run(
        &self,
        shell: &Shell,
        command: &str,
        reporter: &Reporter,
    ) -> Result<String, ProcessError> {
        if command.trim().is_empty() {
            return Err(ProcessError::EmptyCommand);
        }
        let process_id = Uuid::new_v4().to_string();
        let (tx, mut stop) = oneshot::channel();
        self.running
            .lock()
            .expect("process registry poisoned")
            .insert(
                process_id.clone(),
                Control {
                    branch_id: reporter.branch_id(),
                    stop: Some(tx),
                },
            );
        let mut execution = Execution::new(self, reporter, process_id, shell.max_output_chars());
        reporter
            .process_event(
                &execution.process_id,
                Data::Started(ProcessStarted {
                    command: command.to_string(),
                    workdir: shell.workdir().display().to_string(),
                }),
            )
            .await?;
        if stop.try_recv().is_ok() {
            return execution.finish(Reason::Stopped, None).await;
        }
        if let Err(error) = execution.spawn(shell, command) {
            execution
                .finish(Reason::Failed, Some(error.to_string()))
                .await?;
            return Err(error.into());
        }
        let (reason, result) = tokio::select! {
            result = execution.follow() => (Reason::Exited, result),
            _ = stop => (Reason::Stopped, Ok(())),
            _ = tokio::time::sleep(shell.timeout()) => (Reason::TimedOut, Ok(())),
        };
        // Stop the group even when logging fails. Reap the leader before acknowledging completion.
        let result = match result {
            Ok(()) if reason != Reason::Exited => match execution.kill_group() {
                Ok(()) => execution.follow().await,
                Err(error) => Err(error.into()),
            },
            result => result,
        };
        if let Err(error) = result {
            execution.kill_group()?;
            execution.reap().await?;
            execution
                .finish(Reason::Failed, Some(error.to_string()))
                .await?;
            return Err(error);
        }
        execution.finish(reason, None).await
    }

    /// An idempotent stop request, not a claim that the process has already exited.
    pub fn stop(&self, process_id: &str, branch_id: u64) -> Result<String, ProcessError> {
        let mut running = self.running.lock().expect("process registry poisoned");
        let control = running
            .get_mut(process_id)
            .filter(|control| control.branch_id == branch_id)
            .ok_or_else(|| ProcessError::NotRunning(process_id.to_string()))?;
        if let Some(stop) = control.stop.take() {
            stop.send(())
                .map_err(|_| ProcessError::NotRunning(process_id.to_string()))?;
        }
        Ok(format!("stop requested: {process_id}"))
    }
}

struct Execution<'a> {
    processes: &'a Processes,
    reporter: &'a Reporter,
    process_id: String,
    started: Instant,
    child: Option<Child>,
    group: Option<i32>,
    stdout: Option<ChildStdout>,
    stderr: Option<ChildStderr>,
    stdout_tail: OutputTail,
    stderr_tail: OutputTail,
    status: Option<ExitStatus>,
    finished: bool,
}

impl<'a> Execution<'a> {
    fn new(
        processes: &'a Processes,
        reporter: &'a Reporter,
        process_id: String,
        max_chars: usize,
    ) -> Self {
        Self {
            processes,
            reporter,
            process_id,
            started: Instant::now(),
            child: None,
            group: None,
            stdout: None,
            stderr: None,
            stdout_tail: OutputTail::new(max_chars),
            stderr_tail: OutputTail::new(max_chars),
            status: None,
            finished: false,
        }
    }

    fn spawn(&mut self, shell: &Shell, command: &str) -> io::Result<()> {
        let mut child = Command::new("sh")
            .args(["-c", command])
            .current_dir(shell.workdir())
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        self.group = child.id().map(|pid| pid as i32);
        self.stdout = child.stdout.take();
        self.stderr = child.stderr.take();
        self.child = Some(child);
        Ok(())
    }

    async fn follow(&mut self) -> Result<(), ProcessError> {
        let mut stdout = [0; CHUNK_BYTES];
        let mut stderr = [0; CHUNK_BYTES];
        while self.status.is_none() || self.stdout.is_some() || self.stderr.is_some() {
            tokio::select! {
                read = async { self.stdout.as_mut().expect("stdout open").read(&mut stdout).await }, if self.stdout.is_some() => {
                    let count = read?;
                    if count == 0 { self.stdout = None; } else { self.output(Stream::Stdout, &stdout[..count]).await?; }
                }
                read = async { self.stderr.as_mut().expect("stderr open").read(&mut stderr).await }, if self.stderr.is_some() => {
                    let count = read?;
                    if count == 0 { self.stderr = None; } else { self.output(Stream::Stderr, &stderr[..count]).await?; }
                }
                status = self.child.as_mut().expect("process spawned").wait(), if self.status.is_none() => {
                    self.status = Some(status?);
                    // Background descendants must not outlive a task-owned command.
                    self.kill_group()?;
                }
            }
        }
        Ok(())
    }

    async fn output(&mut self, stream: Stream, bytes: &[u8]) -> Result<(), ReportError> {
        let output = if stream == Stream::Stdout {
            &mut self.stdout_tail
        } else {
            &mut self.stderr_tail
        };
        let offset = output.total;
        output.append(bytes);
        self.reporter
            .process_event(
                &self.process_id,
                Data::Output(ProcessOutput {
                    stream: stream.into(),
                    offset,
                    data: bytes.to_vec(),
                }),
            )
            .await
    }

    fn kill_group(&mut self) -> io::Result<()> {
        let Some(group) = self.group else {
            return Ok(());
        };
        // SAFETY: group is the positive PID returned by our child, started in its own group.
        let result = unsafe { libc::kill(-group, libc::SIGKILL) };
        if result != 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::ESRCH) {
                return Err(error);
            }
        }
        self.group = None;
        Ok(())
    }

    async fn reap(&mut self) -> io::Result<()> {
        if self.status.is_none() {
            if let Some(child) = &mut self.child {
                self.status = Some(child.wait().await?);
            }
        }
        Ok(())
    }

    fn outcome(&self, reason: Reason, error: Option<String>) -> ProcessFinished {
        ProcessFinished {
            reason: reason.into(),
            exit_code: self.status.and_then(|status| status.code()),
            signal: self.status.and_then(|status| status.signal()),
            duration_ms: self.started.elapsed().as_millis() as u64,
            error,
        }
    }

    async fn finish(
        &mut self,
        reason: Reason,
        error: Option<String>,
    ) -> Result<String, ProcessError> {
        let finished = self.outcome(reason, error);
        // Never enqueue a second terminal event if the persistence acknowledgement is lost.
        self.finished = true;
        self.reporter
            .process_event(&self.process_id, Data::Finished(finished.clone()))
            .await?;
        Ok(json!({
            "process_id": self.process_id, "reason": reason.as_str_name(),
            "exit_code": finished.exit_code, "signal": finished.signal,
            "stdout": self.stdout_tail.text(), "stderr": self.stderr_tail.text(),
        })
        .to_string())
    }
}

impl Drop for Execution<'_> {
    fn drop(&mut self) {
        let error = self.kill_group().err().map(|error| error.to_string());
        if !self.finished {
            self.reporter.process_cancelled(
                &self.process_id,
                Data::Finished(self.outcome(Reason::Cancelled, error)),
            );
        }
        self.processes
            .running
            .lock()
            .expect("process registry poisoned")
            .remove(&self.process_id);
        // Child::kill_on_drop handles the leader; Tokio reaps it after cancellation.
    }
}

struct OutputTail {
    bytes: Vec<u8>,
    total: u64,
    max_chars: usize,
}

impl OutputTail {
    fn new(max_chars: usize) -> Self {
        Self {
            bytes: Vec::new(),
            total: 0,
            max_chars,
        }
    }

    fn append(&mut self, bytes: &[u8]) {
        self.total += bytes.len() as u64;
        self.bytes.extend_from_slice(bytes);
        let remove = self
            .bytes
            .len()
            .saturating_sub(self.max_chars.saturating_mul(4));
        self.bytes.drain(..remove);
    }

    fn text(&self) -> String {
        tail(&String::from_utf8_lossy(&self.bytes), self.max_chars)
    }
}
