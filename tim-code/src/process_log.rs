//! Bounded reads from the space journal, using event IDs as resumable cursors.

use serde_json::json;
use tim_client::api::process_finished::Reason;
use tim_client::api::process_output::Stream;
use tim_client::api::space_event::Data as Event;
use tim_client::api::task_process_event::Data;
use tim_client::client::{TimClient, TimClientError};

const PAGE_EVENTS: u32 = 100;
const MAX_CHUNKS: usize = 4;

pub async fn read(
    mut client: TimClient,
    process_id: &str,
    cursor: u64,
) -> Result<String, TimClientError> {
    let page = client.get_timeline(cursor.max(1), PAGE_EVENTS).await?;
    let mut next_cursor = cursor.max(1);
    let mut events = Vec::new();
    let mut chunks = 0;
    let mut has_more = page.events.len() == PAGE_EVENTS as usize;
    for event in page.events {
        // Stop before consuming another event so resuming never loses output.
        if chunks == MAX_CHUNKS {
            has_more = true;
            break;
        }
        let Some(meta) = event.metadata else {
            continue;
        };
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
                chunks += 1;
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
    Ok(json!({"process_id": process_id, "events": events, "next_cursor": next_cursor, "has_more": has_more}).to_string())
}
