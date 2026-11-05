use std::collections::HashMap;

use chrono::SecondsFormat;
use chrono::TimeZone;
use chrono::Utc;
use thiserror::Error;
use tokio_stream::StreamExt;

use tim_client::api::space_event::Data as Event;
use tim_client::api::EventCallAbility;
use tim_client::api::EventCallAbilityOutcome;
use tim_client::api::EventNewMessage;
use tim_client::api::Timite;
use tim_client::client::TimClient;
use tim_client::client::TimClientError;
use tim_llm::llm::LlmItem;

const TIMELINE_PAGE_SIZE: u32 = 128;

pub(super) struct Memory {
    client: TimClient,
}

#[derive(Debug, Error)]
pub(super) enum MemoryError {
    #[error("timeline fetch failed: {0}")]
    Timeline(#[from] TimClientError),
}

impl Memory {
    pub(super) fn new(client: TimClient) -> Self {
        Self { client }
    }

    pub(super) async fn context(&mut self) -> Result<Vec<LlmItem>, MemoryError> {
        let self_id = self.client.timite_id();
        let mut messages = Vec::new();
        let mut names = HashMap::new();
        let mut stream = Box::pin(self.client.timeline_stream(TIMELINE_PAGE_SIZE));
        while let Some(page) = stream.next().await {
            let page = page?;
            Self::collect_usernames(&mut names, &page.timites);
            for event in &page.events {
                if let Some(message) = Self::render_event(event, &names, self_id) {
                    messages.push(message);
                }
            }
        }
        Ok(messages)
    }

    fn collect_usernames(names: &mut HashMap<u64, String>, timites: &[Timite]) {
        for timite in timites {
            let username = timite.username.trim();
            if username.is_empty() {
                continue;
            }
            names.insert(timite.id, username.to_string());
        }
    }

    fn render_event(
        event: &tim_client::api::SpaceEvent,
        names: &HashMap<u64, String>,
        my_timite_id: u64,
    ) -> Option<LlmItem> {
        let emitted_at = event
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.emitted_at.as_ref());
        match &event.data {
            Some(Event::EventNewMessage(msg)) => {
                Self::render_new_message(msg, emitted_at, names, my_timite_id)
            }
            Some(Event::EventCallAbility(call)) => {
                Self::render_call_ability(call, names, my_timite_id)
            }
            Some(Event::EventCallAbilityOutcome(outcome)) => {
                Self::render_call_outcome(outcome, my_timite_id)
            }
            Some(Event::EventTimiteConnected(_)) => None,
            Some(Event::EventTimiteDisconnected(_)) => None,
            Some(Event::EventTypingStarted(_)) => None,
            Some(Event::EventTypingStopped(_)) => None,
            Some(Event::EventTaskStarted(_)) => None,
            Some(Event::EventTaskStep(_)) => None,
            Some(Event::EventTaskToolCall(_)) => None,
            Some(Event::EventTaskModelEvent(_)) => None,
            Some(Event::EventTaskFinished(_)) => None,
            None => None,
        }
    }

    fn render_new_message(
        new_message: &EventNewMessage,
        emitted_at: Option<&prost_types::Timestamp>,
        names: &HashMap<u64, String>,
        my_timite_id: u64,
    ) -> Option<LlmItem> {
        let message = new_message.message.as_ref()?;
        let content = message.content.trim();
        if content.is_empty() {
            return None;
        }
        let timestamp = Self::format_emitted_at(emitted_at).unwrap_or_else(|| "-".to_string());
        let username = Self::timite_username(message.sender_id, names);
        let content = format!("[{timestamp}:{username}]: {content}");
        Some(Self::item_for_timite(
            Some(message.sender_id),
            my_timite_id,
            content,
        ))
    }

    fn render_call_ability(
        call: &EventCallAbility,
        names: &HashMap<u64, String>,
        my_timite_id: u64,
    ) -> Option<LlmItem> {
        let payload = call.call_ability.as_ref()?;
        let sender = Self::format_timite_label(payload.sender_id, names);
        let content = format!(
            "CallAbility:{} sender={} payload={}",
            payload.name.trim(),
            sender,
            payload.payload.trim()
        );
        Some(Self::item_for_timite(
            Some(payload.sender_id),
            my_timite_id,
            content,
        ))
    }

    fn render_call_outcome(
        outcome: &EventCallAbilityOutcome,
        my_timite_id: u64,
    ) -> Option<LlmItem> {
        let payload = outcome.call_ability_outcome.as_ref()?;
        let mut parts = Vec::new();
        if let Some(data) = payload
            .payload
            .as_ref()
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
        {
            parts.push(format!("payload={data}"));
        }
        if let Some(err) = payload
            .error
            .as_ref()
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
        {
            parts.push(format!("error={err}"));
        }
        let mut line = format!("CallAbilityOutcome:id={}", payload.call_ability_id);
        if !parts.is_empty() {
            line.push(' ');
            line.push_str(&parts.join(" "));
        }
        Some(Self::item_for_timite(None, my_timite_id, line))
    }

    fn format_emitted_at(emitted_at: Option<&prost_types::Timestamp>) -> Option<String> {
        let ts = emitted_at?;
        Utc.timestamp_opt(ts.seconds, ts.nanos as u32)
            .single()
            .map(|dt| dt.to_rfc3339_opts(SecondsFormat::Secs, true))
    }

    fn timite_username(timite_id: u64, names: &HashMap<u64, String>) -> String {
        names
            .get(&timite_id)
            .map(|username| username.to_string())
            .unwrap_or_else(|| format!("timite {}", timite_id))
    }

    fn format_timite_label(timite_id: u64, names: &HashMap<u64, String>) -> String {
        let username = Self::timite_username(timite_id, names);
        format!("[{}]", username)
    }

    fn item_for_timite(timite_id: Option<u64>, my_timite_id: u64, content: String) -> LlmItem {
        match timite_id {
            Some(id) if id == my_timite_id => LlmItem::Assistant(content),
            _ => LlmItem::User(content),
        }
    }
}
