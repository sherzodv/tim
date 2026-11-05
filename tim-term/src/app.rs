use std::collections::HashMap;
use std::time::Duration;
use std::time::Instant;

use ratatui::style::Style;
use tui_textarea::TextArea;

use tim_client::api::space_event::Data as EventData;
use tim_client::api::{Activity, CallAbility, CallAbilityOutcome, Message, SpaceEvent, Timite};

/// How long a activity such as "typing" stays visible after it was received.
pub const ACTIVITY_TTL: Duration = Duration::from_secs(5);
/// Minimal pause between own "typing" activities.
const TYPING_INTERVAL: Duration = Duration::from_secs(3);

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

/// A transient state another timite reported, shown until `until`.
struct LiveActivity {
    nick: String,
    kind: String,
    payload: String,
    until: Instant,
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
    activities: HashMap<u64, LiveActivity>,
    last_typing: Option<Instant>,
    pub my_timite_id: u64,
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
            activities: HashMap::new(),
            last_typing: None,
            my_timite_id,
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
        self.last_typing = None;
        content
    }

    /// True when a "typing" activity should go out now.
    pub fn typing_due(&mut self) -> bool {
        if self.input.lines().iter().all(|line| line.is_empty()) {
            return false;
        }
        let now = Instant::now();
        if self
            .last_typing
            .is_some_and(|last| now.duration_since(last) < TYPING_INTERVAL)
        {
            return false;
        }
        self.last_typing = Some(now);
        true
    }

    /// Activities still in effect, as "nick kind", sorted.
    pub fn active_activities(&self) -> Vec<String> {
        let now = Instant::now();
        let mut active: Vec<String> = self
            .activities
            .values()
            .filter(|activity| activity.until > now)
            .map(|activity| match activity.payload.is_empty() {
                true => format!("{} {}", activity.nick, activity.kind),
                false => format!("{} {}: {}", activity.nick, activity.kind, activity.payload),
            })
            .collect();
        active.sort_unstable();
        active
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
                EventData::EventActivity(es) => {
                    if let Some(activity) = es.activity {
                        self.activity_received(activity);
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

    fn activity_received(&mut self, activity: Activity) {
        if activity.timite_id == self.my_timite_id {
            return;
        }
        let nick = self
            .timite_nick_cache
            .get(&activity.timite_id)
            .cloned()
            .unwrap_or_else(|| format!("user-{}", activity.timite_id));
        self.activities.insert(
            activity.timite_id,
            LiveActivity {
                nick,
                kind: activity.kind,
                payload: activity.payload,
                until: Instant::now() + ACTIVITY_TTL,
            },
        );
    }

    pub fn add_timite_to_cache(&mut self, timite: &Timite) {
        self.timite_nick_cache
            .insert(timite.id, timite.nick.clone());
    }
}
