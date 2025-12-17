use std::collections::HashMap;

use crate::client::{
    CallAbility, CallAbilityOutcome, EventData, Message, SpaceEvent, Timite, TimiteAbilities,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Insert,
}

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

impl TimelineItem {
    #[allow(dead_code)]
    pub fn timestamp(&self) -> u64 {
        match self {
            TimelineItem::Message { timestamp, .. }
            | TimelineItem::TimiteConnected { timestamp, .. }
            | TimelineItem::TimiteDisconnected { timestamp, .. }
            | TimelineItem::AbilityCall { timestamp, .. }
            | TimelineItem::AbilityOutcome { timestamp, .. } => *timestamp,
        }
    }
}

pub struct App {
    pub running: bool,
    pub input_mode: InputMode,
    pub input: String,
    pub cursor_position: usize,
    pub timeline: Vec<TimelineItem>,
    pub timeline_scroll: usize,
    pub online_timites: HashMap<u64, Timite>,
    pub timite_nick_cache: HashMap<u64, String>,
    pub abilities: Vec<TimiteAbilities>,
    pub my_timite_id: u64,
    pub my_nick: String,
    pub show_help: bool,
}

impl App {
    pub fn new(my_timite_id: u64, my_nick: String) -> Self {
        let mut timite_nick_cache = HashMap::new();
        timite_nick_cache.insert(my_timite_id, my_nick.clone());
        Self {
            running: true,
            input_mode: InputMode::Normal,
            input: String::new(),
            cursor_position: 0,
            timeline: Vec::new(),
            timeline_scroll: 0,
            online_timites: HashMap::new(),
            timite_nick_cache,
            abilities: Vec::new(),
            my_timite_id,
            my_nick,
            show_help: false,
        }
    }

    pub fn quit(&mut self) {
        self.running = false;
    }

    pub fn toggle_help(&mut self) {
        self.show_help = !self.show_help;
    }

    pub fn enter_insert_mode(&mut self) {
        self.input_mode = InputMode::Insert;
    }

    pub fn enter_normal_mode(&mut self) {
        self.input_mode = InputMode::Normal;
    }

    pub fn move_cursor_left(&mut self) {
        let cursor_moved_left = self.cursor_position.saturating_sub(1);
        self.cursor_position = self.clamp_cursor(cursor_moved_left);
    }

    pub fn move_cursor_right(&mut self) {
        let cursor_moved_right = self.cursor_position.saturating_add(1);
        self.cursor_position = self.clamp_cursor(cursor_moved_right);
    }

    pub fn move_cursor_up(&mut self) {
        let (line, col) = self.cursor_line_col();
        if line == 0 {
            return;
        }
        let prev_line_start = self.line_start(line - 1);
        let prev_line_len = self.line_len(line - 1);
        let new_col = col.min(prev_line_len);
        self.cursor_position = prev_line_start + new_col;
    }

    pub fn move_cursor_down(&mut self) {
        let (line, col) = self.cursor_line_col();
        let line_count = self.input_line_count();
        if line >= line_count.saturating_sub(1) {
            return;
        }
        let next_line_start = self.line_start(line + 1);
        let next_line_len = self.line_len(line + 1);
        let new_col = col.min(next_line_len);
        self.cursor_position = next_line_start + new_col;
    }

    pub fn enter_char(&mut self, c: char) {
        let index = self.byte_index();
        self.input.insert(index, c);
        self.move_cursor_right();
    }

    pub fn paste(&mut self, text: &str) {
        // Normalize line endings: \r\n -> \n, \r -> \n
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        for c in normalized.chars() {
            self.enter_char(c);
        }
    }

    pub fn delete_char(&mut self) {
        if self.cursor_position == 0 {
            return;
        }
        let current_index = self.cursor_position;
        let from_left = current_index - 1;

        let before_char = self.input.chars().take(from_left);
        let after_char = self.input.chars().skip(current_index);

        self.input = before_char.chain(after_char).collect();
        self.move_cursor_left();
    }

    pub fn take_input(&mut self) -> String {
        let input = std::mem::take(&mut self.input);
        self.cursor_position = 0;
        input
    }

    fn clamp_cursor(&self, new_cursor_pos: usize) -> usize {
        new_cursor_pos.clamp(0, self.input.chars().count())
    }

    fn byte_index(&self) -> usize {
        self.input
            .char_indices()
            .map(|(i, _)| i)
            .nth(self.cursor_position)
            .unwrap_or(self.input.len())
    }

    pub fn input_line_count(&self) -> usize {
        if self.input.is_empty() {
            return 1;
        }
        // Count newlines + 1, since .lines() doesn't count trailing newline
        self.input.chars().filter(|&c| c == '\n').count() + 1
    }

    pub fn cursor_line_col(&self) -> (usize, usize) {
        let mut line = 0;
        let mut col = 0;
        for (i, c) in self.input.chars().enumerate() {
            if i == self.cursor_position {
                break;
            }
            if c == '\n' {
                line += 1;
                col = 0;
            } else {
                col += 1;
            }
        }
        (line, col)
    }

    fn line_start(&self, target_line: usize) -> usize {
        let mut line = 0;
        for (i, c) in self.input.chars().enumerate() {
            if line == target_line {
                return i;
            }
            if c == '\n' {
                line += 1;
            }
        }
        self.input.chars().count()
    }

    fn line_len(&self, target_line: usize) -> usize {
        self.input
            .lines()
            .nth(target_line)
            .map(|l| l.chars().count())
            .unwrap_or(0)
    }

    pub fn timeline_line_count(&self) -> usize {
        self.timeline
            .iter()
            .map(|item| match item {
                TimelineItem::Message { content, .. } => content.lines().count().max(1),
                _ => 1,
            })
            .sum()
    }

    pub fn scroll_up(&mut self) {
        self.timeline_scroll = self.timeline_scroll.saturating_sub(1);
    }

    pub fn scroll_down(&mut self) {
        let max_scroll = self.timeline_line_count().saturating_sub(1);
        self.timeline_scroll = (self.timeline_scroll + 1).min(max_scroll);
    }

    pub fn scroll_to_bottom(&mut self) {
        self.timeline_scroll = self.timeline_line_count().saturating_sub(1);
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

    pub fn set_abilities(&mut self, abilities: Vec<TimiteAbilities>) {
        self.abilities = abilities;
    }

    pub fn add_timite_to_cache(&mut self, timite: &Timite) {
        self.timite_nick_cache
            .insert(timite.id, timite.nick.clone());
    }
}
