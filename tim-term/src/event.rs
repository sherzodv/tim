use crossterm::event::{self, Event as CrosstermEvent, KeyEvent, KeyEventKind};
use tokio::sync::mpsc;

use crate::error::Result;
use tim_client::api::SpaceEvent;

#[derive(Debug)]
pub enum AppEvent {
    Key(KeyEvent),
    Paste(String),
    Resize,
    Space(SpaceEvent),
}

pub struct EventHandler {
    rx: mpsc::UnboundedReceiver<AppEvent>,
    tx: mpsc::UnboundedSender<AppEvent>,
}

impl EventHandler {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();

        let input_tx = tx.clone();
        std::thread::spawn(move || loop {
            let Ok(event) = event::read() else {
                break;
            };
            let app_event = match event {
                CrosstermEvent::Key(key) if key.kind == KeyEventKind::Press => AppEvent::Key(key),
                CrosstermEvent::Paste(text) => AppEvent::Paste(text),
                CrosstermEvent::Resize(_, _) => AppEvent::Resize,
                _ => continue,
            };
            if input_tx.send(app_event).is_err() {
                break;
            }
        });

        Self { rx, tx }
    }

    pub fn sender(&self) -> mpsc::UnboundedSender<AppEvent> {
        self.tx.clone()
    }

    pub async fn next(&mut self) -> Result<AppEvent> {
        self.rx
            .recv()
            .await
            .ok_or_else(|| std::io::Error::other("event channel closed").into())
    }
}
