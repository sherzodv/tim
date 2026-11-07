//! Bounded reads from the space journal, using event IDs as resumable cursors.

use serde::Deserialize;
use serde_json::json;
use tim_client::api::process_finished::Reason;
use tim_client::api::process_output::Stream;
use tim_client::api::space_event::Data as Event;
use tim_client::api::task_process_event::Data;
use tim_client::client::{TimClient, TimClientError};

const PAGE_EVENTS: u32 = 100;
const MAX_OUTPUT_BYTES: usize = 32 * 1024;

/// Numeric offsets begin a new snapshot; continuation cursors retain its upper bound.
#[derive(Deserialize)]
#[serde(untagged)]
pub enum Cursor {
    Start(u64),
    Page { offset: u64, through: u64 },
}

impl Default for Cursor {
    fn default() -> Self {
        Self::Start(0)
    }
}

pub async fn read(
    mut client: TimClient,
    process_id: &str,
    cursor: Cursor,
) -> Result<String, TimClientError> {
    let (offset, through) = match cursor {
        Cursor::Start(offset) => {
            let latest = client.get_timeline(0, 1).await?;
            let through = latest
                .events
                .last()
                .and_then(|event| event.metadata.as_ref())
                .map_or(0, |meta| meta.id);
            (offset.max(1), through)
        }
        Cursor::Page { offset, through } => (offset.max(1), through),
    };
    let page = client.get_timeline(offset, PAGE_EVENTS).await?;
    let mut next_cursor = offset;
    let mut events = Vec::new();
    let mut output_bytes = 0;
    let mut has_more = page.events.len() == PAGE_EVENTS as usize && offset <= through;
    for event in page.events {
        let Some(meta) = event.metadata else {
            continue;
        };
        if meta.id > through {
            has_more = false;
            break;
        }
        next_cursor = meta.id.saturating_add(1);
        if meta.branch_id != client.branch_id() {
            continue;
        }
        let Some(Event::EventTaskProcessEvent(event)) = event.data else {
            continue;
        };
        let Some(event) = event.task_process_event else {
            continue;
        };
        if event.process_id != process_id || event.timite_id != client.timite_id() {
            continue;
        }
        let value = match event.data {
            Some(Data::Started(started)) => json!({
                "event_id": meta.id, "type": "started", "command": started.command,
                "workdir": started.workdir, "background": started.background,
            }),
            Some(Data::Output(output)) => {
                if output_bytes + output.data.len() > MAX_OUTPUT_BYTES {
                    next_cursor = meta.id; // This chunk belongs to the next page.
                    has_more = true;
                    break;
                }
                output_bytes += output.data.len();
                json!({
                    "event_id": meta.id, "type": "output",
                    "stream": Stream::try_from(output.stream).ok().map(|s| s.as_str_name()),
                    "offset": output.offset, "bytes": output.data.len(),
                    "text": String::from_utf8_lossy(&output.data),
                })
            }
            Some(Data::Finished(finished)) => json!({
                "event_id": meta.id, "type": "finished",
                "reason": Reason::try_from(finished.reason).ok().map(|r| r.as_str_name()),
                "exit_code": finished.exit_code, "signal": finished.signal,
                "duration_ms": finished.duration_ms, "error": finished.error,
            }),
            None => continue,
        };
        events.push(value);
    }
    has_more = has_more && next_cursor <= through;
    if !has_more {
        next_cursor = next_cursor.max(through.saturating_add(1));
    }
    Ok(json!({
        "process_id": process_id, "events": events,
        "next_cursor": { "offset": next_cursor, "through": through },
        "has_more": has_more,
    })
    .to_string())
}
