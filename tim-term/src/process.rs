//! Reconstructs a process from persisted events. Byte offsets expose missing output.

use chrono::Local;
use tim_client::api::process_finished::Reason;
use tim_client::api::process_output::Stream;
use tim_client::api::task_process_event::Data;
use tim_client::api::{ProcessFinished, ProcessStarted};

#[derive(Debug, Clone)]
pub struct Process {
    pub timite_id: u64,
    pub task_id: u64,
    pub process_id: String,
    pub branch_id: u64,
    pub timestamp: u64,
    pub interrupted: bool,
    pub stop_call_id: Option<u64>,
    pub stop_message: String,
    started: Option<ProcessStarted>,
    finished: Option<ProcessFinished>,
    stdout: Output,
    stderr: Output,
}

impl Process {
    pub fn new(
        timite_id: u64,
        task_id: u64,
        process_id: String,
        branch_id: u64,
        timestamp: u64,
    ) -> Self {
        Self {
            timite_id,
            task_id,
            process_id,
            branch_id,
            timestamp,
            interrupted: false,
            stop_call_id: None,
            stop_message: String::new(),
            started: None,
            finished: None,
            stdout: Output::default(),
            stderr: Output::default(),
        }
    }

    pub fn record(&mut self, data: Data) {
        self.interrupted = false;
        match data {
            Data::Started(started) => self.started = Some(started),
            Data::Output(output) => match Stream::try_from(output.stream) {
                Ok(Stream::Stdout) => self.stdout.append(output.offset, &output.data),
                Ok(Stream::Stderr) => self.stderr.append(output.offset, &output.data),
                _ => {}
            },
            Data::Finished(finished) => {
                self.finished = Some(finished);
                self.stop_message.clear();
            }
        }
    }

    pub fn status(&self) -> &'static str {
        let Some(finished) = &self.finished else {
            return if self.interrupted || self.started.is_none() {
                "unknown"
            } else {
                "running"
            };
        };
        match Reason::try_from(finished.reason) {
            Ok(Reason::Exited) if finished.exit_code == Some(0) => "succeeded",
            Ok(Reason::Exited | Reason::Failed) => "failed",
            Ok(Reason::Stopped) => "stopped",
            Ok(Reason::TimedOut) => "timed out",
            Ok(Reason::Cancelled) => "cancelled",
            _ => "unknown",
        }
    }

    pub fn label(&self) -> String {
        let command = self
            .started
            .as_ref()
            .map_or("process", |started| started.command.as_str());
        let preview: String = command
            .lines()
            .next()
            .unwrap_or("")
            .chars()
            .take(45)
            .collect();
        let duration = self
            .duration_ms()
            .map(|ms| format!(" · {}.{:03}s", ms / 1000, ms % 1000))
            .unwrap_or_default();
        format!(
            "{}{duration}{} · {preview}",
            self.status(),
            if self.started.as_ref().is_some_and(|s| s.background) {
                " · background"
            } else {
                ""
            }
        )
    }

    fn duration_ms(&self) -> Option<u64> {
        self.finished
            .as_ref()
            .map(|finished| finished.duration_ms)
            .or_else(|| {
                (self.status() == "running").then(|| {
                    (Local::now().timestamp_millis().max(0) as u64).saturating_sub(self.timestamp)
                })
            })
    }

    pub fn details(&self) -> String {
        let mut text = format!(
            "Process: {}\nTask: {}\nStatus: {}\n",
            self.process_id,
            self.task_id,
            self.status()
        );
        if let Some(started) = &self.started {
            text.push_str(&format!(
                "Lifetime: {}\nDirectory: {}\nCommand:\n{}\n",
                if started.background {
                    "session (background)"
                } else {
                    "task"
                },
                started.workdir,
                started.command
            ));
        }
        if let Some(duration) = self.duration_ms() {
            text.push_str(&format!("Duration: {duration} ms\n"));
        }
        if self.interrupted && self.finished.is_none()
            || self.finished.as_ref().is_some_and(|finished| {
                matches!(
                    Reason::try_from(finished.reason),
                    Ok(Reason::Failed | Reason::Cancelled)
                )
            })
        {
            text.push_str("Output may be incomplete: execution or reporting was interrupted.\n");
        }
        if let Some(finished) = &self.finished {
            if let Some(code) = finished.exit_code {
                text.push_str(&format!("Exit code: {code}\n"));
            }
            if let Some(signal) = finished.signal {
                text.push_str(&format!("Signal: {signal}\n"));
            }
            if let Some(error) = &finished.error {
                text.push_str(&format!("Error: {error}\n"));
            }
        }
        if !self.stop_message.is_empty() {
            text.push_str(&format!("{}\n", self.stop_message));
        }
        text.push_str(&format!(
            "\nstdout{}\n{}\n\nstderr{}\n{}",
            self.stdout.note(),
            String::from_utf8_lossy(&self.stdout.bytes),
            self.stderr.note(),
            String::from_utf8_lossy(&self.stderr.bytes)
        ));
        text
    }
}

#[derive(Debug, Clone, Default)]
struct Output {
    bytes: Vec<u8>,
    next_offset: u64,
    incomplete: bool,
}

impl Output {
    fn append(&mut self, offset: u64, data: &[u8]) {
        if offset != self.next_offset {
            self.incomplete = true;
        }
        self.next_offset = offset + data.len() as u64;
        self.bytes.extend_from_slice(data);
    }

    fn note(&self) -> &'static str {
        if self.incomplete {
            " (incomplete: missing or out-of-order chunks)"
        } else {
            ""
        }
    }
}
