//! Tracing must never write over the alternate-screen terminal.

use std::io::{self, Write};
use tokio::sync::watch;

/// Each formatted event replaces the previous notice; reconnect loops cannot fill a queue.
pub struct Writer {
    tx: watch::Sender<String>,
    bytes: Vec<u8>,
}

impl Writer {
    pub fn new(tx: watch::Sender<String>) -> Self {
        Self {
            tx,
            bytes: Vec::new(),
        }
    }
}

impl Write for Writer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        let notice = String::from_utf8_lossy(&self.bytes)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if !notice.is_empty() {
            self.tx.send_replace(notice);
        }
    }
}
