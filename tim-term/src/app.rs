use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::collections::HashSet;

use ratatui::style::Style;
use tui_textarea::TextArea;

use tim_client::api::space_event::Data as EventData;
use tim_client::api::{
    CallAbility, CallAbilityOutcome, Message, SpaceEvent, TaskFinished, TaskModelEvent,
    TaskStarted, TaskStep, TaskToolCall, Timite,
};

use crate::llm::LlmCall;
use crate::process::Process;
use tim_client::api::TaskProcessEvent;

#[derive(Debug, Clone)]
pub enum TimelineItem {
    LlmCall(Box<LlmCall>),
    Process(Box<Process>),
    Message {
        sender: String,
        content: String,
    },
    TimiteConnected {
        username: String,
    },
    TimiteDisconnected {
        username: String,
    },
    AbilityCall {
        caller: String,
        ability_name: String,
    },
    AbilityOutcome {
        ability_name: String,
        success: bool,
    },
    TaskStarted {
        username: String,
        task: String,
    },
    TaskFinished {
        username: String,
        task: String,
        error: Option<String>,
        duration_ms: Option<u64>,
    },
    TaskToolCall {
        username: String,
        tool: String,
        arguments: String,
        /// The output, or the error when `success` is false.
        outcome: String,
        success: bool,
    },
}

/// A task another timite reported and has not finished yet.
struct LiveTask {
    timite_id: u64,
    username: String,
    task: String,
    step: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Compose,
    Browse,
    Inspect,
    Branches,
    Processes,
}

#[derive(Default)]
struct BranchView {
    input: TextArea<'static>,
    scroll_offset: usize,
    selected_item: Option<usize>,
}

pub struct App {
    pub branch_id: u64,
    pub branches: BTreeSet<u64>,
    pub selected_branch: usize,
    pub timeline_event_ids: Vec<u64>,
    events: BTreeMap<u64, SpaceEvent>,
    branch_views: HashMap<u64, BranchView>,
    pub focus: Focus,
    pub selected_item: Option<usize>,
    pub inspector_offset: usize,
    pub inspector_tab: usize,
    pub inspector_return: Focus,
    llm_calls: HashMap<(u64, u64), usize>,
    processes: HashMap<(u64, String), usize>,
    pub notice: Option<String>,
    pub connected: bool,
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
            branch_id: 0,
            branches: BTreeSet::from([0]),
            selected_branch: 0,
            timeline_event_ids: Vec::new(),
            events: BTreeMap::new(),
            branch_views: HashMap::new(),
            focus: Focus::Compose,
            selected_item: None,
            inspector_offset: 0,
            inspector_tab: 0,
            inspector_return: Focus::Browse,
            llm_calls: HashMap::new(),
            processes: HashMap::new(),
            notice: None,
            connected: true,
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

    pub fn toggle_focus(&mut self) {
        self.focus = if self.focus == Focus::Compose {
            if self.selected_item.is_none() {
                self.selected_item = self.timeline.len().checked_sub(1);
            }
            Focus::Browse
        } else {
            Focus::Compose
        };
    }

    pub fn select_item(&mut self, next: bool) {
        if self.timeline.is_empty() {
            return;
        }
        let current = self.selected_item.unwrap_or(self.timeline.len() - 1);
        self.selected_item = Some(if next {
            (current + 1).min(self.timeline.len() - 1)
        } else {
            current.saturating_sub(1)
        });
    }

    pub fn open_inspector(&mut self) {
        if matches!(
            self.selected_item.and_then(|i| self.timeline.get(i)),
            Some(
                TimelineItem::TaskToolCall { .. }
                    | TimelineItem::LlmCall(_)
                    | TimelineItem::Process(_)
            )
        ) {
            self.inspector_return = self.focus;
            self.inspector_offset = 0;
            self.inspector_tab = 0;
            self.focus = Focus::Inspect;
        }
    }

    pub fn selected_llm_call(&self) -> Option<&LlmCall> {
        match self
            .selected_item
            .and_then(|index| self.timeline.get(index))
        {
            Some(TimelineItem::LlmCall(call)) => Some(call),
            _ => None,
        }
    }

    pub fn selected_process(&self) -> Option<&Process> {
        match self
            .selected_item
            .and_then(|index| self.timeline.get(index))
        {
            Some(TimelineItem::Process(process)) => Some(process),
            _ => None,
        }
    }

    pub fn selected_process_mut(&mut self) -> Option<&mut Process> {
        match self
            .selected_item
            .and_then(|index| self.timeline.get_mut(index))
        {
            Some(TimelineItem::Process(process)) => Some(process),
            _ => None,
        }
    }

    /// Current branch only; inherited processes remain visible in the normal timeline.
    pub fn process_items(&self) -> Vec<usize> {
        let mut items: Vec<_> = self
            .timeline
            .iter()
            .enumerate()
            .filter_map(|(index, item)| match item {
                TimelineItem::Process(process) if process.branch_id == self.branch_id => {
                    Some(index)
                }
                _ => None,
            })
            .collect();
        items.sort_by_key(|index| {
            let TimelineItem::Process(process) = &self.timeline[*index] else {
                unreachable!()
            };
            (process.status() != "running", std::cmp::Reverse(*index))
        });
        items
    }

    pub fn open_processes(&mut self) {
        let items = self.process_items();
        if !self
            .selected_item
            .is_some_and(|index| items.contains(&index))
        {
            self.selected_item = items.first().copied();
        }
        self.focus = Focus::Processes;
    }

    pub fn select_process(&mut self, next: bool) {
        let items = self.process_items();
        if items.is_empty() {
            self.selected_item = None;
            return;
        }
        let position = items
            .iter()
            .position(|index| Some(*index) == self.selected_item);
        let position = position.map_or(0, |position| {
            if next {
                (position + 1).min(items.len() - 1)
            } else {
                position.saturating_sub(1)
            }
        });
        self.selected_item = Some(items[position]);
    }

    pub fn open_branches(&mut self) {
        self.selected_branch = self
            .branches
            .iter()
            .position(|id| *id == self.branch_id)
            .unwrap_or(0);
        self.focus = Focus::Branches;
    }

    pub fn selected_message_id(&self) -> Option<u64> {
        let index = self.selected_item?;
        matches!(self.timeline.get(index)?, TimelineItem::Message { .. })
            .then(|| self.timeline_event_ids[index])
    }

    pub fn branch_label(&self, id: u64) -> String {
        if id == 0 {
            return "main (0)".to_string();
        }
        let description = self
            .events
            .get(&id)
            .and_then(|event| match &event.data {
                Some(EventData::EventNewMessage(data)) => data
                    .message
                    .as_ref()
                    .map(|message| message.content.as_str()),
                _ => None,
            })
            .unwrap_or("");
        let preview: String = description
            .lines()
            .next()
            .unwrap_or("")
            .chars()
            .take(60)
            .collect();
        format!("#{id} {preview}")
    }

    pub fn branch_status(&self, branch_id: u64) -> &'static str {
        self.events
            .values()
            .rev()
            .filter(|event| {
                event
                    .metadata
                    .as_ref()
                    .is_some_and(|meta| meta.branch_id == branch_id)
            })
            .find_map(|event| match &event.data {
                Some(EventData::EventTaskStarted(_)) => Some("running"),
                Some(EventData::EventTaskFinished(data)) => {
                    data.task_finished.as_ref().map(|task| {
                        if task.error.is_some() {
                            "failed"
                        } else {
                            "finished"
                        }
                    })
                }
                _ => None,
            })
            .unwrap_or("")
    }

    pub fn parent_branch(&self) -> u64 {
        self.events
            .get(&self.branch_id)
            .and_then(|event| event.metadata.as_ref())
            .map_or(0, |meta| meta.branch_id)
    }

    pub fn switch_branch(&mut self, branch_id: u64) {
        let view = BranchView {
            input: std::mem::replace(&mut self.input, new_input()),
            scroll_offset: self.scroll_offset,
            selected_item: self.selected_item,
        };
        self.branch_views.insert(self.branch_id, view);
        let view = self
            .branch_views
            .remove(&branch_id)
            .unwrap_or_else(|| BranchView {
                input: new_input(),
                ..Default::default()
            });
        self.branch_id = branch_id;
        self.branches.insert(branch_id);
        self.input = view.input;
        self.scroll_offset = view.scroll_offset;
        self.selected_item = view.selected_item;
        self.focus = Focus::Compose;
        self.typing_sent = false;
        self.timeline.clear();
        self.llm_calls.clear();
        self.processes.clear();
        self.timeline_event_ids.clear();
        self.tasks.clear();
        self.typing.clear();
        self.online_timites.clear();
        let events: Vec<_> = self
            .events
            .values()
            .filter(|event| self.visible(event) || is_presence(event))
            .cloned()
            .collect();
        for event in events {
            self.apply_event(event);
        }
    }

    fn visible(&self, event: &SpaceEvent) -> bool {
        let Some(meta) = &event.metadata else {
            return false;
        };
        let mut branch = self.branch_id;
        let mut end = u64::MAX;
        loop {
            if meta.branch_id == branch {
                return meta.id <= end;
            }
            if branch == 0 {
                return false;
            }
            let Some(parent) = self
                .events
                .get(&branch)
                .and_then(|event| event.metadata.as_ref())
            else {
                return false;
            };
            end = branch;
            branch = parent.branch_id;
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
        let Some(meta) = &event.metadata else {
            return;
        };
        self.branches.insert(meta.branch_id);
        if let Some(EventData::EventTaskModelEvent(data)) = &event.data {
            if !data
                .task_model_event
                .as_ref()
                .is_some_and(|event| event.engine == "llm" && event.llm_call_id.is_some())
            {
                return;
            }
        }
        if self.events.insert(meta.id, event.clone()).is_some() {
            return;
        }
        if self.visible(&event) || is_presence(&event) {
            self.apply_event(event);
        }
    }

    fn apply_event(&mut self, event: SpaceEvent) {
        let event_id = event.metadata.as_ref().map_or(0, |meta| meta.id);
        let branch_id = event.metadata.as_ref().map_or(0, |meta| meta.branch_id);
        let previous_len = self.timeline.len();
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
                    self.add_message(message);
                }
            }
            EventData::EventTimiteConnected(e) => {
                if let Some(timite) = e.timite {
                    self.timite_connected(timite);
                }
            }
            EventData::EventTimiteDisconnected(e) => {
                if let Some(timite) = e.timite {
                    self.timite_disconnected(timite);
                }
            }
            EventData::EventCallAbility(e) => {
                if let Some(call) = e.call_ability {
                    self.ability_called(call);
                }
            }
            EventData::EventCallAbilityOutcome(e) => {
                if let Some(outcome) = e.call_ability_outcome {
                    self.ability_outcome(outcome);
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
                    self.task_started(task);
                }
            }
            EventData::EventTaskStep(e) => {
                if let Some(step) = e.task_step {
                    self.task_step(step);
                }
            }
            EventData::EventTaskFinished(e) => {
                if let Some(task) = e.task_finished {
                    self.task_finished(task);
                }
            }
            EventData::EventTaskToolCall(e) => {
                if let Some(call) = e.task_tool_call {
                    self.task_tool_call(call);
                }
            }
            EventData::EventTaskProcessEvent(data) => {
                if let Some(event) = data.task_process_event {
                    self.process_event(event, branch_id, timestamp);
                }
            }
            EventData::EventTaskModelEvent(data) => {
                if let Some(event) = data.task_model_event {
                    self.model_event(event, event_id, timestamp);
                }
            }
        }
        if self.timeline.len() > previous_len {
            self.timeline_event_ids.push(event_id);
        }
    }

    fn model_event(&mut self, event: TaskModelEvent, event_id: u64, timestamp: u64) {
        let Some(call_id) = event.llm_call_id else {
            return;
        };
        let key = (event.task_id, call_id);
        let index = match self.llm_calls.get(&key) {
            Some(index) => *index,
            None => {
                let index = self.timeline.len();
                self.timeline
                    .push(TimelineItem::LlmCall(Box::new(LlmCall::new(
                        event.timite_id,
                        event.task_id,
                        call_id,
                        timestamp,
                    ))));
                self.llm_calls.insert(key, index);
                index
            }
        };
        if let TimelineItem::LlmCall(call) = &mut self.timeline[index] {
            call.record(event_id, timestamp, event.payload);
        }
    }

    fn process_event(&mut self, event: TaskProcessEvent, branch_id: u64, timestamp: u64) {
        let Some(data) = event.data else {
            return;
        };
        let key = (event.task_id, event.process_id.clone());
        let index = match self.processes.get(&key) {
            Some(index) => *index,
            None => {
                let index = self.timeline.len();
                self.timeline
                    .push(TimelineItem::Process(Box::new(Process::new(
                        event.timite_id,
                        event.task_id,
                        event.process_id,
                        branch_id,
                        timestamp,
                    ))));
                self.processes.insert(key, index);
                index
            }
        };
        if let TimelineItem::Process(process) = &mut self.timeline[index] {
            process.record(data);
        }
        if self.focus == Focus::Processes
            && self.selected_item.is_none()
            && branch_id == self.branch_id
        {
            self.selected_item = Some(index);
        }
    }

    fn username(&self, timite_id: u64) -> String {
        self.usernames
            .get(&timite_id)
            .cloned()
            .unwrap_or_else(|| format!("user-{timite_id}"))
    }

    fn add_message(&mut self, message: Message) {
        self.typing.remove(&message.sender_id);
        self.timeline.push(TimelineItem::Message {
            sender: self.username(message.sender_id),
            content: message.content,
        });
    }

    fn timite_connected(&mut self, timite: Timite) {
        let username = timite.username.clone();
        self.usernames.insert(timite.id, username.clone());
        self.online_timites.insert(timite.id, timite);
        self.timeline
            .push(TimelineItem::TimiteConnected { username });
    }

    fn timite_disconnected(&mut self, timite: Timite) {
        for item in &mut self.timeline {
            if let TimelineItem::Process(process) = item {
                if process.timite_id == timite.id {
                    process.interrupted = true;
                }
            }
            if let TimelineItem::LlmCall(call) = item {
                if call.timite_id == timite.id {
                    call.interrupted = true;
                }
            }
        }
        self.online_timites.remove(&timite.id);
        self.typing.remove(&timite.id);
        self.tasks.retain(|_, task| task.timite_id != timite.id);
        self.timeline.push(TimelineItem::TimiteDisconnected {
            username: timite.username,
        });
    }

    fn ability_called(&mut self, call: CallAbility) {
        self.timeline.push(TimelineItem::AbilityCall {
            caller: self.username(call.sender_id),
            ability_name: call.name,
        });
    }

    fn ability_outcome(&mut self, outcome: CallAbilityOutcome) {
        for item in &mut self.timeline {
            if let TimelineItem::Process(process) = item {
                if process.stop_call_id == Some(outcome.call_ability_id) {
                    if let Some(error) = &outcome.error {
                        process.stop_message = format!("Stop failed: {error}");
                    }
                    process.stop_call_id = None;
                }
            }
        }
        self.timeline.push(TimelineItem::AbilityOutcome {
            ability_name: format!("call-{}", outcome.call_ability_id),
            success: outcome.error.is_none(),
        });
    }

    fn task_started(&mut self, task: TaskStarted) {
        let username = self.username(task.timite_id);
        self.timeline.push(TimelineItem::TaskStarted {
            username: username.clone(),
            task: task.task.clone(),
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

    fn task_finished(&mut self, finished: TaskFinished) {
        for item in &mut self.timeline {
            if let TimelineItem::Process(process) = item {
                if process.task_id == finished.task_id {
                    process.interrupted = true;
                }
            }
            if let TimelineItem::LlmCall(call) = item {
                if call.task_id == finished.task_id {
                    call.interrupted = true;
                }
            }
        }
        let task = self
            .tasks
            .remove(&finished.task_id)
            .map(|task| task.task)
            .unwrap_or_else(|| format!("task-{}", finished.task_id));
        self.timeline.push(TimelineItem::TaskFinished {
            username: self.username(finished.timite_id),
            task,
            error: finished.error,
            duration_ms: finished.duration_ms,
        });
    }

    fn task_tool_call(&mut self, call: TaskToolCall) {
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
        });
    }

    pub fn add_timite_to_cache(&mut self, timite: &Timite) {
        self.usernames.insert(timite.id, timite.username.clone());
    }
}

fn is_presence(event: &SpaceEvent) -> bool {
    matches!(
        &event.data,
        Some(EventData::EventTimiteConnected(_) | EventData::EventTimiteDisconnected(_))
    )
}
