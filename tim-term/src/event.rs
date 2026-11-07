use crossterm::event::{self, Event as CrosstermEvent, KeyEvent, KeyEventKind};
use tokio::sync::mpsc;

use crate::error::Result;
use tim_client::api::SpaceEvent;

#[derive(Debug)]
pub enum AppEvent {
    Key(KeyEvent),
    Paste(String),
    Redraw,
    Notice(String),
    Space(SpaceEvent),
}

pub struct EventHandler {
    rx: mpsc::UnboundedReceiver<AppEvent>,
    logs: tokio::sync::watch::Receiver<String>,
    tx: mpsc::UnboundedSender<AppEvent>,
}

impl EventHandler {
    pub fn new(logs: tokio::sync::watch::Receiver<String>) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();

        let input_tx = tx.clone();
        std::thread::spawn(move || loop {
            let Ok(event) = event::read() else {
                break;
            };
            let app_event = match event {
                CrosstermEvent::Key(key) if key.kind == KeyEventKind::Press => AppEvent::Key(key),
                CrosstermEvent::Paste(text) => AppEvent::Paste(text),
                CrosstermEvent::Resize(_, _) => AppEvent::Redraw,
                _ => continue,
            };
            if input_tx.send(app_event).is_err() {
                break;
            }
        });

        Self { rx, tx, logs }
    }

    pub fn sender(&self) -> mpsc::UnboundedSender<AppEvent> {
        self.tx.clone()
    }

    pub async fn next(&mut self) -> Result<AppEvent> {
        tokio::select! {
            Ok(()) = self.logs.changed() => Ok(AppEvent::Notice(self.logs.borrow_and_update().clone())),
            event = self.rx.recv() => event.ok_or_else(|| std::io::Error::other("event channel closed").into()),
            _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => Ok(AppEvent::Redraw),
        }
    }
}
