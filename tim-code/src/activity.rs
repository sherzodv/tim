//! Reports what a call is doing as live activity, repeated while the call lasts.

use std::time::Duration;

use tim_client::client::TimClient;
use tokio::sync::watch;
use tokio::time::sleep;
use tracing::warn;

const KIND: &str = "working";
/// Activities are transient on the receiving side, so the current step is re-sent this often.
const HEARTBEAT: Duration = Duration::from_secs(3);

/// Keeps the space informed about one running call; dropping it stops the updates.
pub struct Reporter {
    step: watch::Sender<String>,
}

impl Reporter {
    pub fn start(mut client: TimClient, step: String) -> Self {
        let (tx, mut rx) = watch::channel(step);
        tokio::spawn(async move {
            loop {
                let step = rx.borrow_and_update().clone();
                if let Err(error) = client.send_activity(KIND, &step).await {
                    warn!(%error, "activity update failed");
                }
                tokio::select! {
                    changed = rx.changed() => {
                        if changed.is_err() {
                            return;
                        }
                    }
                    _ = sleep(HEARTBEAT) => {}
                }
            }
        });
        Self { step: tx }
    }

    pub fn step(&self, step: String) {
        let _ = self.step.send(step);
    }
}
