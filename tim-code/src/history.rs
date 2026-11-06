//! Restores coding context from the visible events of a space branch.

use std::collections::HashSet;

use tim_client::api::space_event::Data as Event;
use tim_client::api::SpaceEvent;
use tim_client::api::TaskModelEvent;
use tim_client::client::TimClient;
use tim_client::client::TimClientError;
use tim_llm::llm::LlmItem;

use crate::llm::tool_output;

#[derive(Debug, thiserror::Error)]
pub enum HistoryError {
    #[error(transparent)]
    Client(#[from] TimClientError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("branch contains unfinished tool calls; fork before the interrupted task")]
    UnfinishedTools,
}

#[derive(Default)]
pub struct History {
    pub items: Vec<LlmItem>,
    pub transcript: String,
}

impl History {
    pub async fn load(client: &mut TimClient, source_event_id: u64) -> Result<Self, HistoryError> {
        let mut events = Vec::new();
        let mut offset = 1;
        loop {
            let page = client.get_timeline(offset, 100).await?;
            if page.events.is_empty() {
                break;
            }
            offset = page
                .events
                .last()
                .and_then(|e| e.metadata.as_ref())
                .map_or(offset, |m| m.id + 1);
            events.extend(page.events);
        }
        Self::restore(&events, client.timite_id(), source_event_id)
    }

    fn restore(
        events: &[SpaceEvent],
        timite_id: u64,
        source_event_id: u64,
    ) -> Result<Self, HistoryError> {
        let mut history = Self::default();
        let mut sources = HashSet::new();
        let mut tasks = HashSet::new();
        let mut pending = HashSet::new();
        for event in events {
            let Some(data) = &event.data else {
                continue;
            };
            let id = event.metadata.as_ref().map_or(0, |m| m.id);
            match data {
                Event::EventNewMessage(data) => {
                    if let Some(message) = &data.message {
                        if id < source_event_id && message.sender_id != timite_id {
                            sources.insert(id);
                            history.items.push(LlmItem::User(message.content.clone()));
                            history
                                .transcript
                                .push_str(&format!("\nUser: {}\n", message.content));
                        }
                    }
                }
                Event::EventCallAbility(data) => {
                    if let Some(call) = &data.call_ability {
                        if id < source_event_id
                            && call.name == "code.task"
                            && call.timite_id == timite_id
                        {
                            let payload: serde_json::Value = serde_json::from_str(&call.payload)?;
                            if let Some(description) = payload["description"].as_str() {
                                sources.insert(id);
                                history.items.push(LlmItem::User(description.to_string()));
                                history
                                    .transcript
                                    .push_str(&format!("\nUser: {description}\n"));
                            }
                        }
                    }
                }
                Event::EventTaskStarted(data) => {
                    let Some(task) = &data.task_started else {
                        continue;
                    };
                    if task.timite_id != timite_id {
                        continue;
                    }
                    if sources.contains(&task.source_event_id) {
                        tasks.insert(task.task_id);
                    }
                }
                Event::EventTaskModelEvent(data) => {
                    let Some(event) = &data.task_model_event else {
                        continue;
                    };
                    if !tasks.contains(&event.task_id) {
                        continue;
                    }
                    history.record_model(event, &mut pending)?;
                }
                Event::EventTaskToolCall(data) => {
                    let Some(call) = &data.task_tool_call else {
                        continue;
                    };
                    if !tasks.contains(&call.task_id) || !pending.remove(&call.call_id) {
                        continue;
                    }
                    let output = tool_output(match &call.error {
                        Some(error) => Err(error.clone()),
                        None => Ok(call.output.clone()),
                    });
                    history
                        .transcript
                        .push_str(&format!("\nTool {}: {output}\n", call.tool));
                    history.items.push(LlmItem::ToolOutput {
                        call_id: call.call_id.clone(),
                        output,
                    });
                }
                _ => {}
            }
        }
        if !pending.is_empty() {
            return Err(HistoryError::UnfinishedTools);
        }
        Ok(history)
    }
    fn record_model(
        &mut self,
        event: &TaskModelEvent,
        pending: &mut HashSet<String>,
    ) -> Result<(), HistoryError> {
        let payload: serde_json::Value = serde_json::from_str(&event.payload)?;
        if event.engine == "claude" {
            if let Some(message) = payload.get("message") {
                if let Some(blocks) = message["content"].as_array() {
                    for block in blocks {
                        match block["type"].as_str() {
                            Some("tool_use") => {
                                if let Some(id) = block["id"].as_str() {
                                    pending.insert(id.to_string());
                                }
                            }
                            Some("tool_result") => {
                                if let Some(id) = block["tool_use_id"].as_str() {
                                    pending.remove(id);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                // Claude's user events are tool results; exclude echoed prompts.
                let tool_result = message["content"].as_array().is_some_and(|blocks| {
                    blocks.iter().any(|block| block["type"] == "tool_result")
                });
                if payload["type"] == "assistant" || tool_result {
                    self.transcript.push_str(&format!("\n{message}\n"));
                    self.items.push(LlmItem::Assistant(message.to_string()));
                }
            }
            return Ok(());
        }
        if event.engine != "llm" {
            return Ok(());
        }
        let items = match payload["type"].as_str() {
            Some("response.output_item.done") => payload.get("item").cloned().into_iter().collect(),
            Some("turn") => payload["items"].as_array().cloned().unwrap_or_default(),
            _ => return Ok(()),
        };
        for item in &items {
            if item["type"] == "function_call" {
                if let Some(id) = item["call_id"].as_str() {
                    pending.insert(id.to_string());
                }
            }
        }
        self.transcript.push_str(&format!(
            "\nAssistant: {}\n",
            serde_json::to_string(&items)?
        ));
        self.items.push(LlmItem::Turn(items));
        Ok(())
    }
}
