//! Reports one running task to the space: its start, every step and its finish.

use tim_client::client::TimClient;
use tokio::sync::mpsc;
use tracing::warn;

/// Keeps the space informed about one running task; dropping it reports the finish.
pub struct Reporter {
    steps: mpsc::UnboundedSender<String>,
}

impl Reporter {
    pub fn start(mut client: TimClient, task: String) -> Self {
        let (tx, mut rx) = mpsc::unbounded_channel::<String>();
        tokio::spawn(async move {
            let task_id = match client.send_task_started(&task).await {
                Ok(task_id) => task_id,
                Err(error) => {
                    warn!(%error, "task start report failed");
                    return;
                }
            };
            while let Some(step) = rx.recv().await {
                if let Err(error) = client.send_task_step(task_id, &step).await {
                    warn!(%error, task_id, "task step report failed");
                }
            }
            if let Err(error) = client.send_task_finished(task_id).await {
                warn!(%error, task_id, "task finish report failed");
            }
        });
        Self { steps: tx }
    }

    pub fn step(&self, step: String) {
        let _ = self.steps.send(step);
    }
}
