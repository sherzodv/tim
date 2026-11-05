use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use prost_types::Timestamp;
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::mpsc;
use tracing::error;

use crate::api::space_event::Data as EventData;
use crate::api::space_event::Metadata as EventMetadata;
use crate::api::CallAbility;
use crate::api::CallAbilityOutcome;
use crate::api::EventCallAbility;
use crate::api::EventCallAbilityOutcome;
use crate::api::EventNewMessage;
use crate::api::EventTaskFinished;
use crate::api::EventTaskStarted;
use crate::api::EventTaskStep;
use crate::api::EventTaskToolCall;
use crate::api::EventTimiteConnected;
use crate::api::EventTimiteDisconnected;
use crate::api::EventTypingStarted;
use crate::api::EventTypingStopped;
use crate::api::Message;
use crate::api::SpaceEvent;
use crate::api::SubscribeToSpaceReq;
use crate::api::TaskFinished;
use crate::api::TaskStarted;
use crate::api::TaskStep;
use crate::api::TaskToolCall;
use crate::api::Timite;
use crate::api::TypingStarted;
use crate::api::TypingStopped;
use crate::tim_storage::TimStorage;
use crate::tim_storage::TimStorageError;

/// Live events kept for subscriptions that read slower than the space publishes.
const LIVE_CAPACITY: usize = 1024;
/// Events buffered between a subscription and its gRPC stream.
const SUBSCRIBER_BUFFER: usize = 16;
/// Events read from storage per page while a subscription catches up.
const REPLAY_PAGE: u32 = 100;

#[derive(Debug, thiserror::Error)]
pub enum TimSpaceError {
    #[error("Lock poisoned: {0}")]
    LockPoisoned(String),

    #[error("Timeline error: {0}")]
    Timeline(#[from] TimStorageError),
}

struct Presence {
    timite: Timite,
    subscriptions: usize,
}

struct SpaceState {
    next_id: u64,
    online: HashMap<u64, Presence>,
}

pub struct TimSpace {
    state: Mutex<SpaceState>,
    live: broadcast::Sender<SpaceEvent>,
    storage: Arc<TimStorage>,
}

/// Forwards space events to one subscribed client.
struct Subscription {
    space: Arc<TimSpace>,
    timite_id: u64,
    receive_own_messages: bool,
    /// Id of the last event this subscription has passed; earlier ones are never sent again.
    cursor: u64,
    live: broadcast::Receiver<SpaceEvent>,
    out: mpsc::Sender<SpaceEvent>,
}

fn event_new_message(message: &Message) -> EventData {
    EventData::EventNewMessage(EventNewMessage {
        message: Some(message.clone()),
    })
}

fn event_call_ability_outcome(outcome: &CallAbilityOutcome, sender_id: u64) -> EventData {
    EventData::EventCallAbilityOutcome(EventCallAbilityOutcome {
        call_ability_outcome: Some(outcome.clone()),
        sender_id,
    })
}

fn event_call_ability(call_ability: &CallAbility) -> EventData {
    EventData::EventCallAbility(EventCallAbility {
        call_ability: Some(call_ability.clone()),
    })
}

fn event_timite_connected(timite: &Timite) -> EventData {
    EventData::EventTimiteConnected(EventTimiteConnected {
        timite: Some(timite.clone()),
    })
}

fn event_timite_disconnected(timite: &Timite) -> EventData {
    EventData::EventTimiteDisconnected(EventTimiteDisconnected {
        timite: Some(timite.clone()),
    })
}

fn event_typing_started(timite_id: u64) -> EventData {
    EventData::EventTypingStarted(EventTypingStarted {
        typing_started: Some(TypingStarted { timite_id }),
    })
}

fn event_typing_stopped(timite_id: u64) -> EventData {
    EventData::EventTypingStopped(EventTypingStopped {
        typing_stopped: Some(TypingStopped { timite_id }),
    })
}

fn event_task_started(task_started: TaskStarted) -> EventData {
    EventData::EventTaskStarted(EventTaskStarted {
        task_started: Some(task_started),
    })
}

fn event_task_step(task_step: TaskStep) -> EventData {
    EventData::EventTaskStep(EventTaskStep {
        task_step: Some(task_step),
    })
}

fn event_task_tool_call(task_tool_call: TaskToolCall) -> EventData {
    EventData::EventTaskToolCall(EventTaskToolCall {
        task_tool_call: Some(task_tool_call),
    })
}

fn event_task_finished(task_finished: TaskFinished) -> EventData {
    EventData::EventTaskFinished(EventTaskFinished {
        task_finished: Some(task_finished),
    })
}

fn now_timestamp_ms() -> Timestamp {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    Timestamp {
        seconds: now.as_secs() as i64,
        nanos: (now.subsec_millis() * 1_000_000) as i32,
    }
}

fn event_metadata(id: u64) -> Option<EventMetadata> {
    Some(EventMetadata {
        id,
        emitted_at: Some(now_timestamp_ms()),
    })
}

fn event_id(event: &SpaceEvent) -> u64 {
    event.metadata.as_ref().map(|meta| meta.id).unwrap_or(0)
}

/// Timite whose own doing the event reports; `None` for events every subscriber receives.
fn event_sender_id(event: &SpaceEvent) -> Option<u64> {
    match event.data.as_ref()? {
        EventData::EventNewMessage(data) => data.message.as_ref().map(|msg| msg.sender_id),
        EventData::EventCallAbilityOutcome(data) => Some(data.sender_id),
        EventData::EventTypingStarted(data) => data.typing_started.as_ref().map(|t| t.timite_id),
        EventData::EventTypingStopped(data) => data.typing_stopped.as_ref().map(|t| t.timite_id),
        EventData::EventTaskStarted(data) => data.task_started.as_ref().map(|t| t.timite_id),
        EventData::EventTaskStep(data) => data.task_step.as_ref().map(|t| t.timite_id),
        EventData::EventTaskToolCall(data) => data.task_tool_call.as_ref().map(|t| t.timite_id),
        EventData::EventTaskFinished(data) => data.task_finished.as_ref().map(|t| t.timite_id),
        _ => None,
    }
}

impl TimSpace {
    pub fn new(storage: Arc<TimStorage>) -> Result<TimSpace, TimSpaceError> {
        let max_event_id = storage.fetch_max_event_id()?;
        let (live, _) = broadcast::channel(LIVE_CAPACITY);
        Ok(TimSpace {
            state: Mutex::new(SpaceState {
                next_id: max_event_id + 1,
                online: HashMap::new(),
            }),
            live,
            storage,
        })
    }

    pub fn publish_message(&self, message: &Message) -> Result<(), TimSpaceError> {
        self.append(&mut self.lock_state(), event_new_message(message))
    }

    pub fn publish_call_ability(&self, call_ability: &CallAbility) -> Result<(), TimSpaceError> {
        self.append(&mut self.lock_state(), event_call_ability(call_ability))
    }

    pub fn publish_call_outcome(
        &self,
        outcome: &CallAbilityOutcome,
        sender_id: u64,
    ) -> Result<(), TimSpaceError> {
        self.append(
            &mut self.lock_state(),
            event_call_ability_outcome(outcome, sender_id),
        )
    }

    pub fn publish_typing_started(&self, timite_id: u64) -> Result<(), TimSpaceError> {
        self.append(&mut self.lock_state(), event_typing_started(timite_id))
    }

    pub fn publish_typing_stopped(&self, timite_id: u64) -> Result<(), TimSpaceError> {
        self.append(&mut self.lock_state(), event_typing_stopped(timite_id))
    }

    /// Returns the task id: the id of the event that started it.
    pub fn publish_task_started(&self, timite_id: u64, task: &str) -> Result<u64, TimSpaceError> {
        let mut state = self.lock_state();
        let task_id = state.next_id;
        let task_started = TaskStarted {
            timite_id,
            task_id,
            task: task.to_string(),
        };
        self.append(&mut state, event_task_started(task_started))?;
        Ok(task_id)
    }

    pub fn publish_task_step(&self, task_step: TaskStep) -> Result<(), TimSpaceError> {
        self.append(&mut self.lock_state(), event_task_step(task_step))
    }

    pub fn publish_task_tool_call(&self, tool_call: TaskToolCall) -> Result<(), TimSpaceError> {
        self.append(&mut self.lock_state(), event_task_tool_call(tool_call))
    }

    pub fn publish_task_finished(&self, task_finished: TaskFinished) -> Result<(), TimSpaceError> {
        self.append(&mut self.lock_state(), event_task_finished(task_finished))
    }

    pub fn subscribe(
        self: &Arc<Self>,
        req: &SubscribeToSpaceReq,
        timite: Timite,
    ) -> Result<mpsc::Receiver<SpaceEvent>, TimSpaceError> {
        let (sender, receiver) = mpsc::channel(SUBSCRIBER_BUFFER);
        let subscription = self.attach(req, timite, sender)?;
        tokio::spawn(subscription.run());
        Ok(receiver)
    }

    pub fn timeline(&self, offset: u64, size: u32) -> Result<Vec<SpaceEvent>, TimSpaceError> {
        self.storage.timeline(offset, size).map_err(Into::into)
    }

    fn lock_state(&self) -> MutexGuard<'_, SpaceState> {
        self.state.lock().expect("space state lock poisoned")
    }

    /// Assigns the next id, persists the event and hands it to live subscriptions.
    /// Runs under the state lock so ids, storage and live delivery share one order.
    fn append(&self, state: &mut SpaceState, data: EventData) -> Result<(), TimSpaceError> {
        let event = SpaceEvent {
            metadata: event_metadata(state.next_id),
            data: Some(data),
        };
        self.storage.store_space_event(&event)?;
        state.next_id += 1;
        // Fails only without live subscriptions; the event is already in storage.
        let _ = self.live.send(event);
        Ok(())
    }

    fn attach(
        self: &Arc<Self>,
        req: &SubscribeToSpaceReq,
        timite: Timite,
        out: mpsc::Sender<SpaceEvent>,
    ) -> Result<Subscription, TimSpaceError> {
        let mut state = self.lock_state();
        let presence = state.online.entry(timite.id).or_insert_with(|| Presence {
            timite: timite.clone(),
            subscriptions: 0,
        });
        presence.subscriptions += 1;
        if presence.subscriptions == 1 {
            self.append(&mut state, event_timite_connected(&timite))?;
        }
        let head = state.next_id - 1;
        Ok(Subscription {
            space: self.clone(),
            timite_id: timite.id,
            receive_own_messages: req.receive_own_messages,
            cursor: req.after_id.map_or(head, |after_id| after_id.min(head)),
            live: self.live.subscribe(),
            out,
        })
    }

    fn detach(&self, timite_id: u64) -> Result<(), TimSpaceError> {
        let mut state = self.lock_state();
        let Entry::Occupied(mut entry) = state.online.entry(timite_id) else {
            return Ok(());
        };
        entry.get_mut().subscriptions -= 1;
        if entry.get().subscriptions > 0 {
            return Ok(());
        }
        let presence = entry.remove();
        self.append(&mut state, event_timite_disconnected(&presence.timite))
    }
}

impl Subscription {
    async fn run(mut self) {
        if let Err(error) = self.forward_live().await {
            error!("subscription of timite {} failed: {error}", self.timite_id);
        }
        if let Err(error) = self.space.detach(self.timite_id) {
            error!("detach of timite {} failed: {error}", self.timite_id);
        }
    }

    /// Ends when the client stops reading or the space is dropped.
    async fn forward_live(&mut self) -> Result<(), TimSpaceError> {
        if !self.replay().await? {
            return Ok(());
        }
        loop {
            let received = tokio::select! {
                received = self.live.recv() => received,
                _ = self.out.closed() => return Ok(()),
            };
            let delivered = match received {
                Ok(event) => self.forward(event).await,
                Err(RecvError::Lagged(_)) => self.replay().await?,
                Err(RecvError::Closed) => return Ok(()),
            };
            if !delivered {
                return Ok(());
            }
        }
    }

    /// Forwards events between the cursor and the storage head.
    async fn replay(&mut self) -> Result<bool, TimSpaceError> {
        loop {
            let events = self.space.storage.timeline(self.cursor + 1, REPLAY_PAGE)?;
            if events.is_empty() {
                return Ok(true);
            }
            for event in events {
                if !self.forward(event).await {
                    return Ok(false);
                }
            }
        }
    }

    /// Returns `false` once the client stopped reading.
    async fn forward(&mut self, event: SpaceEvent) -> bool {
        let id = event_id(&event);
        if id <= self.cursor {
            return true;
        }
        self.cursor = id;
        if self.is_own(&event) {
            return true;
        }
        self.out.send(event).await.is_ok()
    }

    fn is_own(&self, event: &SpaceEvent) -> bool {
        !self.receive_own_messages && event_sender_id(event) == Some(self.timite_id)
    }
}
