use std::collections::HashMap;

use thiserror::Error;

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

    pub(super) async fn context(&mut self) -> Result<Option<String>, MemoryError> {
        let mut offset = 0;
        let mut buf = String::new();
        let mut names = HashMap::new();
        loop {
            let page = self.client.get_timeline(offset, TIMELINE_PAGE_SIZE).await?;
            if page.events.is_empty() {
                break;
            }
            self.expand_names(&mut names, &page.timites);
            for event in &page.events {
                if let Some(line) = self.render_event(event, &names) {
                    buf.push_str(&line);
                    buf.push('\n');
                }
            }
            let last_id = page
                .events
                .last()
                .and_then(|event| event.metadata.as_ref().map(|meta| meta.id));
            if let Some(id) = last_id {
                offset = id.saturating_add(1);
            } else {
                offset = offset.saturating_add(page.events.len() as u64);
            }
            if page.events.len() < TIMELINE_PAGE_SIZE as usize {
                break;
            }
        }
        if buf.is_empty() {
            Ok(None)
        } else {
            Ok(Some(buf.trim_end().to_string()))
        }
    }

    fn expand_names(&self, names: &mut HashMap<u64, String>, timites: &[Timite]) {
        for timite in timites {
            let nick = timite.nick.trim();
            if nick.is_empty() {
                continue;
            }
            names.insert(timite.id, nick.to_string());
        }
    }

    fn render_event(
        &self,
        event: &crate::tim_client::SpaceEvent,
        names: &HashMap<u64, String>,
    ) -> Option<String> {
        match &event.data {
            Some(Event::EventNewMessage(msg)) => self.render_new_message(msg, names),
            Some(Event::EventCallAbility(call)) => self.render_call_ability(call, names),
            Some(Event::EventCallAbilityOutcome(outcome)) => self.render_call_outcome(outcome),
            None => None,
        }
    }

    fn render_new_message(
        &self,
        new_message: &EventNewMessage,
        names: &HashMap<u64, String>,
    ) -> Option<String> {
        let message = new_message.message.as_ref()?;
        let content = message.content.trim();
        if content.is_empty() {
            return None;
        }
        let header = self.format_timite_label(message.sender_id, names);
        Some(format!("{header}: {content}"))
    }

    fn render_call_ability(
        &self,
        call: &EventCallAbility,
        names: &HashMap<u64, String>,
    ) -> Option<String> {
        let payload = call.call_ability.as_ref()?;
        let sender = self.format_timite_label(payload.sender_id, names);
        Some(format!(
            "CallAbility:{} sender={} payload={}",
            payload.name.trim(),
            sender,
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

    fn format_timite_label(&self, timite_id: u64, names: &HashMap<u64, String>) -> String {
        if timite_id == self.client.timite_id() {
            "[Me]".to_string()
        } else if let Some(nick) = names.get(&timite_id) {
            format!("[{}]", nick)
        } else {
            format!("[timite {}]", timite_id)
        }
    }
}
