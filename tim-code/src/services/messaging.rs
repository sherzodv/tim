use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::api::command_content::Value as CommandContentValue;
use crate::api::space_update::Event as SpaceUpdateEvent;
use crate::api::{
    CommandContent, CommandEntry, CommandRole, Message, SessionHelp, SessionStatus, SessionTheme,
    SpaceNewMessage, SpaceUpdate, Theme, WorkspaceEntriesClear,
};

pub const DEFAULT_STATUS: &str = "Ready";
pub const DEFAULT_HELP: &str =
    "Type `HELP` for available commands. Press `Esc` to cancel current input.";
pub const SYSTEM_SENDER_ID: &str = "tim-code";
pub const ASSISTANT_SENDER_ID: &str = "assistant";

pub fn command_entry(command: String) -> CommandEntry {
    CommandEntry {
        id: next_entry_id(),
        role: CommandRole::Command as i32,
        content: Some(CommandContent {
            value: Some(CommandContentValue::Text(command)),
        }),
    }
}

pub fn output_entry_text(text: impl Into<String>) -> CommandEntry {
    CommandEntry {
        id: next_entry_id(),
        role: CommandRole::Output as i32,
        content: Some(CommandContent {
            value: Some(CommandContentValue::Text(text.into())),
        }),
    }
}

pub fn output_entry_html(html: String) -> CommandEntry {
    CommandEntry {
        id: next_entry_id(),
        role: CommandRole::Output as i32,
        content: Some(CommandContent {
            value: Some(CommandContentValue::Html(html)),
        }),
    }
}

pub fn help_html() -> String {
    r#"
<div class="help-block">
	<h3>Available commands</h3>
	<dl class="help-list">
		<dt>HELP</dt>
		<dd>Display this help overview.</dd>
		<dt>CLEAR</dt>
		<dd>Reset the workspace log.</dd>
		<dt>THEME &lt;night|day&gt;</dt>
		<dd>Switch the active theme.</dd>
	</dl>
	<p class="help-hint">Commands are case-insensitive. Try “THEME night”.</p>
</div>
"#
    .trim()
    .to_string()
}

pub struct SessionUpdates;

impl SessionUpdates {
    pub fn append_entry(id: String, sender_id: &str, entry: CommandEntry) -> SpaceUpdate {
        SpaceUpdate {
            id,
            event: Some(SpaceUpdateEvent::SpaceNewMessage(SpaceNewMessage {
                message: Some(Message {
                    sender_id: sender_id.to_string(),
                    entry: Some(entry),
                }),
            })),
        }
    }

    pub fn workspace_cleared(id: String) -> SpaceUpdate {
        SpaceUpdate {
            id,
            event: Some(SpaceUpdateEvent::WorkspaceEntriesClear(
                WorkspaceEntriesClear {},
            )),
        }
    }

    pub fn status(id: String, status: impl Into<String>) -> SpaceUpdate {
        SpaceUpdate {
            id,
            event: Some(SpaceUpdateEvent::SessionStatus(SessionStatus {
                status: status.into(),
            })),
        }
    }

    pub fn help(id: String, help: impl Into<String>) -> SpaceUpdate {
        SpaceUpdate {
            id,
            event: Some(SpaceUpdateEvent::SessionHelp(SessionHelp {
                help: help.into(),
            })),
        }
    }

    pub fn theme(id: String, theme: Theme) -> SpaceUpdate {
        SpaceUpdate {
            id,
            event: Some(SpaceUpdateEvent::SessionTheme(SessionTheme {
                theme: theme as i32,
            })),
        }
    }
}

fn next_entry_id() -> i64 {
    static ENTRY_COUNTER: AtomicU64 = AtomicU64::new(0);
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_millis() as u64;
    let counter = ENTRY_COUNTER.fetch_add(1, Ordering::Relaxed) % 1000;
    (millis * 1000 + counter) as i64
}
