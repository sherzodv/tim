use std::time::Duration;

use crossterm::event::{self, Event as CrosstermEvent, KeyEvent};
use tokio::sync::mpsc;

use crate::client::SpaceEvent;
use crate::error::Result;

#[derive(Debug)]
pub enum AppEvent {
    Key(KeyEvent),
    Paste(String),
    Tick,
    Space(SpaceEvent),
}

pub struct EventHandler {
    rx: mpsc::UnboundedReceiver<AppEvent>,
    _tx: mpsc::UnboundedSender<AppEvent>,
}

impl EventHandler {
    pub fn new(tick_rate: Duration) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();

        let key_tx = tx.clone();
        std::thread::spawn(move || {
            loop {
                if event::poll(tick_rate).unwrap_or(false) {
                    match event::read() {
                        Ok(CrosstermEvent::Key(key)) => {
                            if key_tx.send(AppEvent::Key(key)).is_err() {
                                break;
                            }
                        }
                        Ok(CrosstermEvent::Paste(text)) => {
                            if key_tx.send(AppEvent::Paste(text)).is_err() {
                                break;
                            }
                        }
                        _ => {}
                    }
                } else if key_tx.send(AppEvent::Tick).is_err() {
                    break;
                }
            }
        });

        Self { rx, _tx: tx }
    }

    pub fn sender(&self) -> mpsc::UnboundedSender<AppEvent> {
        self._tx.clone()
    }

    pub async fn next(&mut self) -> Result<AppEvent> {
        self.rx
            .recv()
            .await
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "event channel closed").into())
    }
}
