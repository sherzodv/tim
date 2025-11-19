use crate::tim_client::tim_api::{EventCallAbility, EventCallAbilityOutcome, EventNewMessage};
use crate::tim_client::{Event, TimClient, TimClientError};
use thiserror::Error;

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

    pub(super) async fn context(&mut self) -> Result<Option<String>, MemoryError> {
        let mut offset = 0;
        let mut buf = String::new();
        loop {
            let page = self.client.get_timeline(offset, TIMELINE_PAGE_SIZE).await?;
            if page.is_empty() {
                break;
            }
            for event in &page {
                if let Some(line) = self.render_event(event) {
                    buf.push_str(&line);
                    buf.push('\n');
                }
            }
            let last_id = page
                .last()
                .and_then(|event| event.metadata.as_ref().map(|meta| meta.id));
            if let Some(id) = last_id {
                offset = id.saturating_add(1);
            } else {
                offset = offset.saturating_add(page.len() as u64);
            }
            if page.len() < TIMELINE_PAGE_SIZE as usize {
                break;
            }
        }
        if buf.is_empty() {
            Ok(None)
        } else {
            Ok(Some(buf.trim_end().to_string()))
        }
    }

    fn render_event(&self, event: &crate::tim_client::SpaceEvent) -> Option<String> {
        match &event.data {
            Some(Event::EventNewMessage(msg)) => self.render_new_message(msg),
            Some(Event::EventCallAbility(call)) => self.render_call_ability(call),
            Some(Event::EventCallAbilityOutcome(outcome)) => self.render_call_outcome(outcome),
            None => None,
        }
    }

    fn render_new_message(&self, new_message: &EventNewMessage) -> Option<String> {
        let message = new_message.message.as_ref()?;
        let content = message.content.trim();
        if content.is_empty() {
            return None;
        }
        let header = if message.sender_id == self.client.timite_id() {
            "Agent"
        } else {
            "Peer"
        };
        Some(format!("{header}: {content}"))
    }

    fn render_call_ability(&self, call: &EventCallAbility) -> Option<String> {
        let payload = call.call_ability.as_ref()?;
        Some(format!(
            "CallAbility:{} sender={} payload={}",
            payload.name.trim(),
            payload.sender_id,
            payload.payload.trim()
        ))
    }

    fn render_call_outcome(&self, outcome: &EventCallAbilityOutcome) -> Option<String> {
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
        Some(line)
    }
}
