//! Managed shell processes. The space is the output log; only tails stay in memory.
//! Unix process groups delimit ownership: commands must stay in their process group (no daemonization).

use std::collections::HashMap;
use std::io;
use std::os::unix::process::ExitStatusExt;
use std::process::{ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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
    #[error("process is not known to this agent in this branch: {0}")]
    NotFound(String),
    #[error("process start was interrupted")]
    StartInterrupted,
    #[error("process supervisor is shutting down")]
    ShuttingDown,
    #[error("process start failed: {0}")]
    Start(String),
}

struct Control {
    branch_id: u64,
    stop: Option<oneshot::Sender<()>>,
    command: String,
    started: Instant,
    outcome: Option<ProcessFinished>,
}

#[derive(Default)]
struct Registry {
    closing: bool,
    records: HashMap<String, Control>,
}

#[derive(Clone, Default)]
pub struct Processes {
    registry: Arc<Mutex<Registry>>,
    background: Arc<Mutex<tokio::task::JoinSet<()>>>,
}

impl Processes {
    pub async fn run(
        &self,
        shell: &Shell,
        command: &str,
        reporter: &Reporter,
    ) -> Result<String, ProcessError> {
        self.execute(shell, command, reporter, None).await
    }

    /// Returns only after start is persisted and the OS process has been spawned.
    pub async fn start(
        &self,
        shell: &Shell,
        command: &str,
        parent: &Reporter,
    ) -> Result<String, ProcessError> {
        if command.trim().is_empty() {
            return Err(ProcessError::EmptyCommand);
        }
        let reporter = parent
            .child(format!("sh.start: {}", crate::text::head(command, 80)))
            .await
            .map_err(ReportError::from)?;
        let processes = self.clone();
        let shell = shell.clone();
        let command = command.to_string();
        let (ready, started) = oneshot::channel();
        {
            let mut background = self.background.lock().expect("process supervisor poisoned");
            while background.try_join_next().is_some() {}
            background.spawn(async move {
                let result = processes
                    .execute(&shell, &command, &reporter, Some(ready))
                    .await;
                let error = result.as_ref().err().map(ToString::to_string);
                if let Err(error) = reporter.finish(error).await {
                    tracing::error!(%error, "failed to finish background process task");
                }
            });
        }
        started
            .await
            .map_err(|_| ProcessError::StartInterrupted)?
            .map_err(ProcessError::Start)
    }

    async fn execute(
        &self,
        shell: &Shell,
        command: &str,
        reporter: &Reporter,
        mut ready: Option<oneshot::Sender<Result<String, String>>>,
    ) -> Result<String, ProcessError> {
        if command.trim().is_empty() {
            return Err(ProcessError::EmptyCommand);
        }
        let background = ready.is_some();
        let process_id = Uuid::new_v4().to_string();
        let (tx, mut stop) = oneshot::channel();
        {
            let mut registry = self.registry.lock().expect("process registry poisoned");
            if registry.closing {
                return Err(ProcessError::ShuttingDown);
            }
            registry.records.insert(
                process_id.clone(),
                Control {
                    branch_id: reporter.branch_id(),
                    stop: Some(tx),
                    command: command.to_string(),
                    started: Instant::now(),
                    outcome: None,
                },
            );
        }
        let mut execution = Execution::new(self, reporter, process_id, shell.max_output_chars());
        let launch = reporter
            .process_event(
                &execution.process_id,
                Data::Started(ProcessStarted {
                    command: command.to_string(),
                    workdir: shell.workdir().display().to_string(),
                    background,
                }),
            )
            .await
            .map_err(ProcessError::from);
        if launch.is_ok() {
            let stopped = stop.try_recv().is_ok();
            let cancelled = ready.as_ref().is_some_and(|ready| ready.is_closed());
            if stopped || cancelled {
                if let Some(ready) = ready {
                    let _ = ready.send(Err("cancelled before spawn".into()));
                }
                return execution
                    .finish(
                        if stopped {
                            Reason::Stopped
                        } else {
                            Reason::Cancelled
                        },
                        None,
                    )
                    .await;
            }
        }
        let launch =
            launch.and_then(|()| execution.spawn(shell, command).map_err(ProcessError::from));
        if let Err(error) = launch {
            let message = error.to_string();
            if let Some(ready) = ready.take() {
                let _ = ready.send(Err(message.clone()));
            }
            execution.finish(Reason::Failed, Some(message)).await?;
            return Err(error);
        }
        if let Some(ready) = ready {
            let response =
                json!({"process_id": execution.process_id, "status": "started", "output_cursor": reporter.task_id()}).to_string();
            if ready.send(Ok(response)).is_err() {
                // The caller was cancelled before receiving the ID: do not leave an orphan.
                execution.kill_group()?;
                execution.reap().await?;
                return execution.finish(Reason::Cancelled, None).await;
            }
        }
        let (reason, result) = tokio::select! {
            result = execution.follow() => (Reason::Exited, result),
            _ = &mut stop => (Reason::Stopped, Ok(())),
            _ = process_timeout((!background).then_some(shell.timeout())) => (Reason::TimedOut, Ok(())),
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

    /// Stop managed processes before dropping the runtime, then abort stalled reporting.
    pub async fn shutdown(&self) {
        {
            let mut registry = self.registry.lock().expect("process registry poisoned");
            registry.closing = true;
            for control in registry.records.values_mut() {
                if let Some(stop) = control.stop.take() {
                    let _ = stop.send(());
                }
            }
        }
        let mut background =
            std::mem::take(&mut *self.background.lock().expect("process supervisor poisoned"));
        if tokio::time::timeout(Duration::from_secs(5), async {
            while background.join_next().await.is_some() {}
        })
        .await
        .is_err()
        {
            background.abort_all();
            while background.join_next().await.is_some() {}
        }
    }

    /// Runtime status; persisted history remains available through sh.output and the terminal.
    pub fn status(&self, process_id: &str, branch_id: u64) -> Result<String, ProcessError> {
        let registry = self.registry.lock().expect("process registry poisoned");
        let control = registry
            .records
            .get(process_id)
            .filter(|c| c.branch_id == branch_id)
            .ok_or_else(|| ProcessError::NotFound(process_id.to_string()))?;
        let finished = control.outcome.as_ref();
        Ok(json!({
            "process_id": process_id, "command": control.command,
            "status": if finished.is_some() { "finished" } else { "running" },
            "reason": finished.and_then(|f| Reason::try_from(f.reason).ok()).map(|r| r.as_str_name()),
            "duration_ms": finished.map_or_else(|| control.started.elapsed().as_millis() as u64, |f| f.duration_ms),
            "exit_code": finished.and_then(|f| f.exit_code),
            "signal": finished.and_then(|f| f.signal),
            "error": finished.and_then(|f| f.error.as_deref()),
        }).to_string())
    }

    /// An idempotent stop request, not a claim that the process has already exited.
    pub fn stop(&self, process_id: &str, branch_id: u64) -> Result<String, ProcessError> {
        let mut registry = self.registry.lock().expect("process registry poisoned");
        let control = registry
            .records
            .get_mut(process_id)
            .filter(|control| control.branch_id == branch_id)
            .ok_or_else(|| ProcessError::NotRunning(process_id.to_string()))?;
        if control.outcome.is_some() {
            return Ok(format!("already finished: {process_id}"));
        }
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

    fn record_outcome(&self, outcome: ProcessFinished) {
        if let Some(control) = self
            .processes
            .registry
            .lock()
            .expect("process registry poisoned")
            .records
            .get_mut(&self.process_id)
        {
            control.outcome = Some(outcome);
            control.stop = None;
        }
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
        self.record_outcome(finished.clone());
        self.finished = true;
        self.reporter
            .process_event(&self.process_id, Data::Finished(finished.clone()))
            .await?;
        Ok(json!({
            "process_id": self.process_id, "reason": reason.as_str_name(),
            "output_cursor": self.reporter.task_id(),
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
            self.record_outcome(self.outcome(Reason::Cancelled, error.clone()));
            self.reporter.process_cancelled(
                &self.process_id,
                Data::Finished(self.outcome(Reason::Cancelled, error)),
            );
        }
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

async fn process_timeout(duration: Option<Duration>) {
    match duration {
        Some(duration) => tokio::time::sleep(duration).await,
        None => std::future::pending().await,
    }
}
