//! Reports one running task to the space: its start, every step and tool call, its finish.

use tim_client::api::TaskToolCall;
use tim_client::client::TimClient;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::warn;

enum Report {
    Step(String),
    ToolCall(TaskToolCall),
}

/// Keeps the space informed about one running task; `finish` reports its end.
pub struct Reporter {
    reports: mpsc::UnboundedSender<Report>,
    worker: JoinHandle<()>,
}

impl Reporter {
    pub fn start(mut client: TimClient, task: String) -> Self {
        let (tx, mut rx) = mpsc::unbounded_channel::<Report>();
        let worker = tokio::spawn(async move {
            let task_id = match client.send_task_started(&task).await {
                Ok(task_id) => task_id,
                Err(error) => {
                    warn!(%error, "task start report failed");
                    return;
                }
            };
            while let Some(report) = rx.recv().await {
                let sent = match report {
                    Report::Step(step) => client.send_task_step(task_id, &step).await,
                    Report::ToolCall(call) => {
                        client
                            .send_task_tool_call(TaskToolCall { task_id, ..call })
                            .await
                    }
                };
                if let Err(error) = sent {
                    warn!(%error, task_id, "task report failed");
                }
            }
            if let Err(error) = client.send_task_finished(task_id).await {
                warn!(%error, task_id, "task finish report failed");
            }
        });
        Self {
            reports: tx,
            worker,
        }
    }

    /// Reports the finish and waits until it is in the log, so a reply sent next comes after it.
    pub async fn finish(self) {
        drop(self.reports);
        let _ = self.worker.await;
    }

    /// What the task is doing now, in a few words.
    pub fn step(&self, step: String) {
        let _ = self.reports.send(Report::Step(step));
    }

    /// A tool call that returned; `task_id` and `timite_id` are filled in on the way.
    pub fn tool_call(&self, tool: String, arguments: String, result: Result<String, String>) {
        let (output, error) = match result {
            Ok(output) => (output, None),
            Err(error) => (String::new(), Some(error)),
        };
        let _ = self.reports.send(Report::ToolCall(TaskToolCall {
            timite_id: 0,
            task_id: 0,
            tool,
            arguments,
            output,
            error,
        }));
    }
}
