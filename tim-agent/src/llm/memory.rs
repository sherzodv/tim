use std::collections::HashMap;

use chrono::SecondsFormat;
use chrono::TimeZone;
use chrono::Utc;
use thiserror::Error;
use tokio_stream::StreamExt;

use crate::llm::llm::LlmInputItem;
use crate::tim_client::tim_api::EventCallAbility;
use crate::tim_client::tim_api::EventCallAbilityOutcome;
use crate::tim_client::tim_api::EventNewMessage;
use crate::tim_client::tim_api::Timite;
use crate::tim_client::Event;
use crate::tim_client::TimClient;
use crate::tim_client::TimClientError;

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

    pub(super) async fn context(&mut self) -> Result<Vec<LlmInputItem>, MemoryError> {
        let self_id = self.client.timite_id();
        let mut messages = Vec::new();
        let mut names = HashMap::new();
        let mut stream = Box::pin(self.client.timeline_stream(TIMELINE_PAGE_SIZE));
        while let Some(page) = stream.next().await {
            let page = page?;
            Self::collect_nicks(&mut names, &page.timites);
            for event in &page.events {
                if let Some(message) = Self::render_event(event, &names, self_id) {
                    messages.push(message);
                }
            }
        }
        Ok(messages)
    }

    fn collect_nicks(names: &mut HashMap<u64, String>, timites: &[Timite]) {
        for timite in timites {
            let nick = timite.nick.trim();
            if nick.is_empty() {
                continue;
            }
            names.insert(timite.id, nick.to_string());
        }
    }

    fn render_event(
        event: &crate::tim_client::SpaceEvent,
        names: &HashMap<u64, String>,
        my_timite_id: u64,
    ) -> Option<LlmInputItem> {
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
            None => None,
        }
    }

    fn render_new_message(
        new_message: &EventNewMessage,
        emitted_at: Option<&prost_types::Timestamp>,
        names: &HashMap<u64, String>,
        my_timite_id: u64,
    ) -> Option<LlmInputItem> {
        let message = new_message.message.as_ref()?;
        let content = message.content.trim();
        if content.is_empty() {
            return None;
        }
        let timestamp = Self::format_emitted_at(emitted_at).unwrap_or_else(|| "-".to_string());
        let nick = Self::timite_nick(message.sender_id, names);
        let role = Self::role_for_timite(Some(message.sender_id), my_timite_id);
        let content = format!("[{timestamp}:{nick}]: {content}");
        Some(LlmInputItem { role, content })
    }

    fn render_call_ability(
        call: &EventCallAbility,
        names: &HashMap<u64, String>,
        my_timite_id: u64,
    ) -> Option<LlmInputItem> {
        let payload = call.call_ability.as_ref()?;
        let sender = Self::format_timite_label(payload.sender_id, names);
        let role = Self::role_for_timite(Some(payload.sender_id), my_timite_id);
        Some(LlmInputItem {
            role,
            content: format!(
                "CallAbility:{} sender={} payload={}",
                payload.name.trim(),
                sender,
                payload.payload.trim()
            ),
        })
    }

    fn render_call_outcome(
        outcome: &EventCallAbilityOutcome,
        my_timite_id: u64,
    ) -> Option<LlmInputItem> {
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
        Some(LlmInputItem {
            role: Self::role_for_timite(None, my_timite_id),
            content: line,
        })
    }

    fn format_emitted_at(emitted_at: Option<&prost_types::Timestamp>) -> Option<String> {
        let ts = emitted_at?;
        Utc.timestamp_opt(ts.seconds, ts.nanos as u32)
            .single()
            .map(|dt| dt.to_rfc3339_opts(SecondsFormat::Secs, true))
    }

    fn timite_nick(timite_id: u64, names: &HashMap<u64, String>) -> String {
        names
            .get(&timite_id)
            .map(|nick| nick.to_string())
            .unwrap_or_else(|| format!("timite {}", timite_id))
    }

    fn format_timite_label(timite_id: u64, names: &HashMap<u64, String>) -> String {
        let nick = Self::timite_nick(timite_id, names);
        format!("[{}]", nick)
    }

    fn role_for_timite(timite_id: Option<u64>, my_timite_id: u64) -> &'static str {
        match timite_id {
            Some(id) if id == my_timite_id => "assistant",
            _ => "user",
        }
    }
}
