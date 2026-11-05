use std::collections::HashMap;

use ratatui::style::Style;
use tui_textarea::TextArea;

use tim_client::api::space_event::Data as EventData;
use tim_client::api::{CallAbility, CallAbilityOutcome, Message, SpaceEvent, Timite};

#[derive(Debug, Clone)]
pub enum TimelineItem {
    Message {
        sender: String,
        content: String,
        timestamp: u64,
    },
    TimiteConnected {
        nick: String,
        timestamp: u64,
    },
    TimiteDisconnected {
        nick: String,
        timestamp: u64,
    },
    AbilityCall {
        caller: String,
        ability_name: String,
        timestamp: u64,
    },
    AbilityOutcome {
        ability_name: String,
        success: bool,
        timestamp: u64,
    },
}

pub struct App {
    pub running: bool,
    pub input: TextArea<'static>,
    pub timeline: Vec<TimelineItem>,
    /// Timeline lines hidden below the viewport; zero follows new events.
    pub scroll_offset: usize,
    /// Viewport height from the last render, used as the page size.
    pub timeline_height: usize,
    pub online_timites: HashMap<u64, Timite>,
    pub timite_nick_cache: HashMap<u64, String>,
    pub my_nick: String,
}

fn new_input() -> TextArea<'static> {
    let mut input = TextArea::default();
    input.set_cursor_line_style(Style::default());
    input.set_placeholder_text("Write a message");
    input
}

impl App {
    pub fn new(my_timite_id: u64, my_nick: String) -> Self {
        let mut timite_nick_cache = HashMap::new();
        timite_nick_cache.insert(my_timite_id, my_nick.clone());
        Self {
            running: true,
            input: new_input(),
            timeline: Vec::new(),
            scroll_offset: 0,
            timeline_height: 0,
            online_timites: HashMap::new(),
            timite_nick_cache,
            my_nick,
        }
    }

    pub fn quit(&mut self) {
        self.running = false;
    }

    pub fn paste(&mut self, text: &str) {
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        self.input.insert_str(normalized);
    }

    pub fn take_input(&mut self) -> String {
        let content = self.input.lines().join("\n");
        self.input = new_input();
        content
    }

    pub fn scroll_up(&mut self, lines: usize) {
        self.scroll_offset += lines;
    }

    pub fn scroll_down(&mut self, lines: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(lines);
    }

    pub fn handle_space_event(&mut self, event: SpaceEvent) {
        let timestamp = event
            .metadata
            .as_ref()
            .and_then(|m| m.emitted_at.as_ref())
            .map(|t| t.seconds as u64 * 1000 + t.nanos as u64 / 1_000_000)
            .unwrap_or(0);

        if let Some(data) = event.data {
            match data {
                EventData::EventNewMessage(msg) => {
                    if let Some(message) = msg.message {
                        self.add_message(message, timestamp);
                    }
                }
                EventData::EventTimiteConnected(tc) => {
                    if let Some(timite) = tc.timite {
                        self.timite_connected(timite, timestamp);
                    }
                }
                EventData::EventTimiteDisconnected(td) => {
                    if let Some(timite) = td.timite {
                        self.timite_disconnected(timite, timestamp);
                    }
                }
                EventData::EventCallAbility(ca) => {
                    if let Some(call) = ca.call_ability {
                        self.ability_called(call, timestamp);
                    }
                }
                EventData::EventCallAbilityOutcome(cao) => {
                    if let Some(outcome) = cao.call_ability_outcome {
                        self.ability_outcome(outcome, timestamp);
                    }
                }
            }
        }
    }

    fn add_message(&mut self, message: Message, timestamp: u64) {
        let sender = self
            .timite_nick_cache
            .get(&message.sender_id)
            .cloned()
            .unwrap_or_else(|| format!("user-{}", message.sender_id));
        self.timeline.push(TimelineItem::Message {
            sender,
            content: message.content,
            timestamp,
        });
    }

    fn timite_connected(&mut self, timite: Timite, timestamp: u64) {
        let nick = timite.nick.clone();
        self.timite_nick_cache.insert(timite.id, nick.clone());
        self.online_timites.insert(timite.id, timite);
        self.timeline
            .push(TimelineItem::TimiteConnected { nick, timestamp });
    }

    fn timite_disconnected(&mut self, timite: Timite, timestamp: u64) {
        self.online_timites.remove(&timite.id);
        self.timeline.push(TimelineItem::TimiteDisconnected {
            nick: timite.nick,
            timestamp,
        });
    }

    fn ability_called(&mut self, call: CallAbility, timestamp: u64) {
        let caller = self
            .timite_nick_cache
            .get(&call.sender_id)
            .cloned()
            .unwrap_or_else(|| format!("user-{}", call.sender_id));
        self.timeline.push(TimelineItem::AbilityCall {
            caller,
            ability_name: call.name,
            timestamp,
        });
    }

    fn ability_outcome(&mut self, outcome: CallAbilityOutcome, timestamp: u64) {
        self.timeline.push(TimelineItem::AbilityOutcome {
            ability_name: format!("call-{}", outcome.call_ability_id),
            success: outcome.error.is_none(),
            timestamp,
        });
    }

    pub fn add_timite_to_cache(&mut self, timite: &Timite) {
        self.timite_nick_cache
            .insert(timite.id, timite.nick.clone());
    }
}
