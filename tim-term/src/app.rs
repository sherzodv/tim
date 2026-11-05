use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;

use ratatui::style::Style;
use tui_textarea::TextArea;

use tim_client::api::space_event::Data as EventData;
use tim_client::api::{
    CallAbility, CallAbilityOutcome, Message, SpaceEvent, TaskFinished, TaskStarted, TaskStep,
    TaskToolCall, Timite,
};

#[derive(Debug, Clone)]
pub enum TimelineItem {
    Message {
        sender: String,
        content: String,
        timestamp: u64,
    },
    TimiteConnected {
        username: String,
        timestamp: u64,
    },
    TimiteDisconnected {
        username: String,
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
    TaskStarted {
        username: String,
        task: String,
        timestamp: u64,
    },
    TaskFinished {
        username: String,
        task: String,
        timestamp: u64,
    },
    TaskToolCall {
        username: String,
        tool: String,
        arguments: String,
        /// The output, or the error when `success` is false.
        outcome: String,
        success: bool,
        timestamp: u64,
    },
}

/// A task another timite reported and has not finished yet.
struct LiveTask {
    timite_id: u64,
    username: String,
    task: String,
    step: Option<String>,
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
    pub usernames: HashMap<u64, String>,
    typing: HashSet<u64>,
    /// Running tasks by task id, in start order.
    tasks: BTreeMap<u64, LiveTask>,
    /// Whether the space was told that this timite is typing.
    typing_sent: bool,
    pub my_timite_id: u64,
    pub my_username: String,
}

fn new_input() -> TextArea<'static> {
    let mut input = TextArea::default();
    input.set_cursor_line_style(Style::default());
    input.set_placeholder_text("Write a message");
    input
}

impl App {
    pub fn new(my_timite_id: u64, my_username: String) -> Self {
        let mut usernames = HashMap::new();
        usernames.insert(my_timite_id, my_username.clone());
        Self {
            running: true,
            input: new_input(),
            timeline: Vec::new(),
            scroll_offset: 0,
            timeline_height: 0,
            online_timites: HashMap::new(),
            usernames,
            typing: HashSet::new(),
            tasks: BTreeMap::new(),
            typing_sent: false,
            my_timite_id,
            my_username,
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

    /// Whether own typing changed with the input: `Some(true)` started, `Some(false)` stopped.
    pub fn typing_transition(&mut self) -> Option<bool> {
        let typing = self.input.lines().iter().any(|line| !line.is_empty());
        if typing == self.typing_sent {
            return None;
        }
        self.typing_sent = typing;
        Some(typing)
    }

    /// What other timites are doing right now, typing first, then tasks in start order.
    pub fn status(&self) -> Vec<String> {
        let mut status: Vec<String> = self
            .typing
            .iter()
            .filter(|id| **id != self.my_timite_id)
            .map(|id| format!("{} typing", self.username(*id)))
            .collect();
        status.sort_unstable();
        status.extend(self.tasks.values().map(|task| {
            let detail = task.step.as_deref().unwrap_or(&task.task);
            format!("{} working: {detail}", task.username)
        }));
        status
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

        let Some(data) = event.data else {
            return;
        };
        match data {
            EventData::EventNewMessage(e) => {
                if let Some(message) = e.message {
                    self.add_message(message, timestamp);
                }
            }
            EventData::EventTimiteConnected(e) => {
                if let Some(timite) = e.timite {
                    self.timite_connected(timite, timestamp);
                }
            }
            EventData::EventTimiteDisconnected(e) => {
                if let Some(timite) = e.timite {
                    self.timite_disconnected(timite, timestamp);
                }
            }
            EventData::EventCallAbility(e) => {
                if let Some(call) = e.call_ability {
                    self.ability_called(call, timestamp);
                }
            }
            EventData::EventCallAbilityOutcome(e) => {
                if let Some(outcome) = e.call_ability_outcome {
                    self.ability_outcome(outcome, timestamp);
                }
            }
            EventData::EventTypingStarted(e) => {
                if let Some(typing) = e.typing_started {
                    self.typing.insert(typing.timite_id);
                }
            }
            EventData::EventTypingStopped(e) => {
                if let Some(typing) = e.typing_stopped {
                    self.typing.remove(&typing.timite_id);
                }
            }
            EventData::EventTaskStarted(e) => {
                if let Some(task) = e.task_started {
                    self.task_started(task, timestamp);
                }
            }
            EventData::EventTaskStep(e) => {
                if let Some(step) = e.task_step {
                    self.task_step(step);
                }
            }
            EventData::EventTaskFinished(e) => {
                if let Some(task) = e.task_finished {
                    self.task_finished(task, timestamp);
                }
            }
            EventData::EventTaskToolCall(e) => {
                if let Some(call) = e.task_tool_call {
                    self.task_tool_call(call, timestamp);
                }
            }
        }
    }

    fn username(&self, timite_id: u64) -> String {
        self.usernames
            .get(&timite_id)
            .cloned()
            .unwrap_or_else(|| format!("user-{timite_id}"))
    }

    fn add_message(&mut self, message: Message, timestamp: u64) {
        self.typing.remove(&message.sender_id);
        self.timeline.push(TimelineItem::Message {
            sender: self.username(message.sender_id),
            content: message.content,
            timestamp,
        });
    }

    fn timite_connected(&mut self, timite: Timite, timestamp: u64) {
        let username = timite.username.clone();
        self.usernames.insert(timite.id, username.clone());
        self.online_timites.insert(timite.id, timite);
        self.timeline.push(TimelineItem::TimiteConnected {
            username,
            timestamp,
        });
    }

    fn timite_disconnected(&mut self, timite: Timite, timestamp: u64) {
        self.online_timites.remove(&timite.id);
        self.typing.remove(&timite.id);
        self.tasks.retain(|_, task| task.timite_id != timite.id);
        self.timeline.push(TimelineItem::TimiteDisconnected {
            username: timite.username,
            timestamp,
        });
    }

    fn ability_called(&mut self, call: CallAbility, timestamp: u64) {
        self.timeline.push(TimelineItem::AbilityCall {
            caller: self.username(call.sender_id),
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

    fn task_started(&mut self, task: TaskStarted, timestamp: u64) {
        let username = self.username(task.timite_id);
        self.timeline.push(TimelineItem::TaskStarted {
            username: username.clone(),
            task: task.task.clone(),
            timestamp,
        });
        self.tasks.insert(
            task.task_id,
            LiveTask {
                timite_id: task.timite_id,
                username,
                task: task.task,
                step: None,
            },
        );
    }

    fn task_step(&mut self, step: TaskStep) {
        if let Some(task) = self.tasks.get_mut(&step.task_id) {
            task.step = Some(step.step);
        }
    }

    fn task_finished(&mut self, finished: TaskFinished, timestamp: u64) {
        let task = self
            .tasks
            .remove(&finished.task_id)
            .map(|task| task.task)
            .unwrap_or_else(|| format!("task-{}", finished.task_id));
        self.timeline.push(TimelineItem::TaskFinished {
            username: self.username(finished.timite_id),
            task,
            timestamp,
        });
    }

    fn task_tool_call(&mut self, call: TaskToolCall, timestamp: u64) {
        let (outcome, success) = match call.error {
            Some(error) => (error, false),
            None => (call.output, true),
        };
        self.timeline.push(TimelineItem::TaskToolCall {
            username: self.username(call.timite_id),
            tool: call.tool,
            arguments: call.arguments,
            outcome,
            success,
            timestamp,
        });
    }

    pub fn add_timite_to_cache(&mut self, timite: &Timite) {
        self.usernames.insert(timite.id, timite.username.clone());
    }
}
