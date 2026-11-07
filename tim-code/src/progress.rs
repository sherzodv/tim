//! Reports one running task to the space: its start, model events, tools and finish.

use std::time::Instant;

use tim_client::api::TaskModelEvent;
use tim_client::api::TaskToolCall;
use tim_client::api::{task_process_event, TaskProcessEvent};
use tim_client::client::TimClient;
use tim_client::client::TimClientError;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

#[derive(Debug, thiserror::Error)]
pub enum ReportError {
    #[error("process event was not persisted: {0}")]
    Process(String),
    #[error(transparent)]
    Client(#[from] TimClientError),
    #[error("task report worker failed: {0}")]
    Worker(#[from] tokio::task::JoinError),
}

enum Report {
    Step(String),
    Finished {
        error: Option<String>,
        duration_ms: u64,
    },
    ToolCall(TaskToolCall),
    ModelEvent(TaskModelEvent),
    ProcessEvent(
        TaskProcessEvent,
        Option<oneshot::Sender<Result<(), String>>>,
    ),
}

/// Keeps the space informed about one running task; `finish` reports its end.
pub struct Reporter {
    client: TimClient,
    task_id: u64,
    branch_id: u64,
    started: Instant,
    reports: mpsc::UnboundedSender<Report>,
    worker: JoinHandle<Result<(), TimClientError>>,
}

impl Reporter {
    pub async fn start(
        mut client: TimClient,
        task: String,
        source_event_id: u64,
    ) -> Result<Self, TimClientError> {
        let branch_id = client.branch_id();
        let started = Instant::now();
        let task_id = client.send_task_started(&task, source_event_id).await?;
        let process_client = client.clone();
        let (tx, mut rx) = mpsc::unbounded_channel::<Report>();
        let worker = tokio::spawn(async move {
            while let Some(report) = rx.recv().await {
                let sent = match report {
                    Report::Finished { error, duration_ms } => {
                        return client
                            .send_task_finished(task_id, error, Some(duration_ms))
                            .await;
                    }
                    Report::ProcessEvent(event, ack) => {
                        let result = client
                            .send_task_process_event(TaskProcessEvent { task_id, ..event })
                            .await;
                        if let Some(ack) = ack {
                            let _ =
                                ack.send(result.as_ref().map(|_| ()).map_err(ToString::to_string));
                        }
                        result
                    }
                    Report::Step(step) => client.send_task_step(task_id, &step).await,
                    Report::ToolCall(call) => {
                        client
                            .send_task_tool_call(TaskToolCall { task_id, ..call })
                            .await
                    }
                    Report::ModelEvent(event) => {
                        client
                            .send_task_model_event(TaskModelEvent { task_id, ..event })
                            .await
                    }
                };
                sent?;
            }
            Ok(())
        });
        Ok(Self {
            client: process_client,
            task_id,
            branch_id,
            started,
            reports: tx,
            worker,
        })
    }

    /// Reports the finish and waits until it is in the log, so a reply sent next comes after it.
    pub async fn finish(self, error: Option<String>) -> Result<(), ReportError> {
        let duration_ms = self.started.elapsed().as_millis() as u64;
        let _ = self.reports.send(Report::Finished { error, duration_ms });
        drop(self.reports);
        self.worker.await??;
        Ok(())
    }

    /// A background process has its own task lifetime, linked to its launching task.
    pub async fn child(&self, task: String) -> Result<Self, TimClientError> {
        Self::start(self.client.clone(), task, self.task_id).await
    }

    pub fn task_id(&self) -> u64 {
        self.task_id
    }

    pub fn client(&self) -> TimClient {
        self.client.clone()
    }

    pub fn branch_id(&self) -> u64 {
        self.branch_id
    }

    /// Wait for persistence before starting the command or reading its next output chunk.
    pub async fn process_event(
        &self,
        process_id: &str,
        data: task_process_event::Data,
    ) -> Result<(), ReportError> {
        let (tx, rx) = oneshot::channel();
        self.reports
            .send(Report::ProcessEvent(
                process_event(process_id, data),
                Some(tx),
            ))
            .map_err(|_| ReportError::Process("report worker closed".into()))?;
        rx.await
            .map_err(|_| ReportError::Process("report worker closed".into()))?
            .map_err(ReportError::Process)
    }

    /// Cancellation cannot await; the terminal event is queued before TaskFinished.
    pub fn process_cancelled(&self, process_id: &str, data: task_process_event::Data) {
        if self
            .reports
            .send(Report::ProcessEvent(process_event(process_id, data), None))
            .is_err()
        {
            tracing::error!(process_id, "failed to queue process cancellation");
        }
    }

    /// What the task is doing now, in a few words.
    pub fn step(&self, step: String) {
        let _ = self.reports.send(Report::Step(step));
    }

    /// A tool call that returned; `task_id` and `timite_id` are filled in on the way.
    pub fn tool_call(
        &self,
        call_id: String,
        tool: String,
        arguments: String,
        result: Result<String, String>,
    ) {
        let (output, error) = match result {
            Ok(output) => (output, None),
            Err(error) => (String::new(), Some(error)),
        };
        let _ = self.reports.send(Report::ToolCall(TaskToolCall {
            timite_id: 0,
            task_id: 0,
            call_id,
            tool,
            arguments,
            output,
            error,
        }));
    }

    /// Records one JSON event from the coding engine in its original order.
    pub fn model_event(&self, engine: &str, payload: String) {
        self.send_model_event(engine, payload, None);
    }

    pub fn llm_event(&self, call_id: u64, payload: String) {
        self.send_model_event("llm", payload, Some(call_id));
    }

    fn send_model_event(&self, engine: &str, payload: String, llm_call_id: Option<u64>) {
        let _ = self.reports.send(Report::ModelEvent(TaskModelEvent {
            timite_id: 0,
            task_id: 0,
            engine: engine.to_string(),
            payload,
            llm_call_id,
        }));
    }
}

fn process_event(process_id: &str, data: task_process_event::Data) -> TaskProcessEvent {
    TaskProcessEvent {
        timite_id: 0,
        task_id: 0,
        process_id: process_id.to_string(),
        data: Some(data),
    }
}
