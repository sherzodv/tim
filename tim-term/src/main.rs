mod app;
mod diagnostics;
mod error;
mod event;
mod llm;
mod process;
mod ui;

use std::io;

use async_trait::async_trait;
use crossterm::{
    event::{DisableBracketedPaste, EnableBracketedPaste, KeyCode, KeyEvent, KeyModifiers},
    execute,
    style::Print,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use tim_client::agent::{Agent, AgentError, AgentRunner};
use tim_client::api::SpaceEvent;
use tim_client::client::{TimClient, TimClientConf};
use tokio::sync::mpsc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::app::{App, Focus};
use crate::error::Result;
use crate::event::{AppEvent, EventHandler};

/// Mouse stays with the terminal so text can be selected; the wheel arrives as arrow keys.
const ENABLE_ALTERNATE_SCROLL: &str = "\x1b[?1007h";
const DISABLE_ALTERNATE_SCROLL: &str = "\x1b[?1007l";

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    let (log_tx, log_rx) = tokio::sync::watch::channel(String::new());
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")))
        .with(
            tracing_subscriber::fmt::layer()
                .without_time()
                .with_ansi(false)
                .with_writer(move || diagnostics::Writer::new(log_tx.clone())),
        )
        .init();

    let endpoint = std::env::var("TIM_ENDPOINT").unwrap_or_else(|_| "http://127.0.0.1:8787".into());
    let username = std::env::var("TIM_USERNAME")
        .unwrap_or_else(|_| whoami::username())
        .to_lowercase();

    let config = TimClientConf {
        endpoint,
        username: username.clone(),
        provider: "tim-term".to_string(),
    };

    tracing::info!("Connecting to Tim server...");
    let mut client = TimClient::new(config).await?;
    let timite_id = client.timite_id();

    let mut app = App::new(timite_id, username);

    // Discover branches and their fork points before starting the all-branch stream.
    let mut cursor = Some(0);
    let mut offset = 1;
    loop {
        let res = client.get_timeline(offset, 100).await?;
        if res.events.is_empty() {
            break;
        }
        for timite in &res.timites {
            app.add_timite_to_cache(timite);
        }
        cursor = res
            .events
            .iter()
            .filter_map(|event| event.metadata.as_ref())
            .map(|metadata| metadata.id)
            .max();
        offset = cursor.unwrap_or(0) + 1;
        for event in res.events {
            app.handle_space_event(event);
        }
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableBracketedPaste,
        Print(ENABLE_ALTERNATE_SCROLL)
    )?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut events = EventHandler::new(log_rx);
    let forwarder = SpaceForwarder {
        tx: events.sender(),
    };
    let mut runner = AgentRunner::new(&client).await;
    tokio::spawn(async move {
        if let Err(error) = runner.start(forwarder, cursor).await {
            tracing::error!(%error, "space subscription ended");
        }
    });

    client = client.in_branch(0);
    let result = run_app(&mut terminal, &mut app, &mut events, &mut client).await;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        Print(DISABLE_ALTERNATE_SCROLL),
        DisableBracketedPaste,
        LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;

    result
}

/// Hands space events to the UI loop; the runner keeps the subscription alive.
struct SpaceForwarder {
    tx: mpsc::UnboundedSender<AppEvent>,
}

#[async_trait]
impl Agent for SpaceForwarder {
    async fn on_space_update(
        &mut self,
        update: &SpaceEvent,
    ) -> std::result::Result<(), AgentError> {
        self.tx
            .send(AppEvent::Space(update.clone()))
            .map_err(|_| AgentError::Agent("ui closed".into()))
    }

    fn receive_own_messages(&self) -> bool {
        true
    }
}

async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    events: &mut EventHandler,
    client: &mut TimClient,
) -> Result<()> {
    while app.running {
        terminal.draw(|f| ui::render(f, app))?;

        match events.next().await? {
            AppEvent::Key(key) => {
                if let Err(error) = handle_key(app, client, key).await {
                    match error {
                        error::Error::Client(_) => app.notice = Some(error.to_string()),
                        error => return Err(error),
                    }
                }
            }
            AppEvent::Notice(notice) => app.notice = Some(notice),
            AppEvent::Paste(text) if app.focus == Focus::Compose => app.paste(&text),
            AppEvent::Paste(_) => {}
            AppEvent::Redraw => {}
            AppEvent::Space(event) => {
                app.notice = None;
                app.handle_space_event(event);
            }
        }
    }

    Ok(())
}

async fn handle_key(app: &mut App, client: &mut TimClient, key: KeyEvent) -> Result<()> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    if ctrl && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('d')) {
        app.quit();
        return Ok(());
    }
    if key.code == KeyCode::Char('x')
        && matches!(app.focus, Focus::Browse | Focus::Inspect | Focus::Processes)
    {
        if let Some(process) = app.selected_process() {
            if process.branch_id != app.branch_id {
                if let Some(process) = app.selected_process_mut() {
                    process.stop_message =
                        format!("Open branch #{} to stop this process", process.branch_id);
                }
                return Ok(());
            }
            if process.status() == "running" && process.stop_call_id.is_none() {
                let payload = serde_json::json!({ "process_id": process.process_id }).to_string();
                let result = client
                    .send_call_ability(process.timite_id, "sh.stop", &payload)
                    .await;
                if let Some(process) = app.selected_process_mut() {
                    match result {
                        Ok(call_id) => {
                            process.stop_call_id = Some(call_id);
                            process.stop_message =
                                "Stop requested; waiting for process completion".into();
                        }
                        Err(error) => process.stop_message = format!("Stop failed: {error}"),
                    }
                }
            }
            return Ok(());
        }
    }
    if key.code == KeyCode::F(4) {
        app.open_processes();
        return Ok(());
    }
    if app.focus == Focus::Processes {
        match key.code {
            KeyCode::Esc => {
                app.focus = Focus::Browse;
                return Ok(());
            }
            KeyCode::Up => {
                app.select_process(false);
                return Ok(());
            }
            KeyCode::Down => {
                app.select_process(true);
                return Ok(());
            }
            KeyCode::Enter => {
                app.open_inspector();
                return Ok(());
            }
            _ => {}
        }
    }
    if key.code == KeyCode::F(2) {
        app.open_branches();
        return Ok(());
    }
    if key.code == KeyCode::F(3) {
        switch_branch(app, client, 0).await?;
        return Ok(());
    }
    if app.focus == Focus::Branches {
        match key.code {
            KeyCode::Esc => app.focus = Focus::Compose,
            KeyCode::Up => app.selected_branch = app.selected_branch.saturating_sub(1),
            KeyCode::Down => {
                app.selected_branch = (app.selected_branch + 1).min(app.branches.len() - 1)
            }
            KeyCode::Enter => {
                if let Some(id) = app.branches.iter().nth(app.selected_branch).copied() {
                    switch_branch(app, client, id).await?;
                }
            }
            _ => {}
        }
        return Ok(());
    }
    if app.focus == Focus::Browse {
        match key.code {
            KeyCode::Char('b') => {
                app.open_branches();
                return Ok(());
            }
            KeyCode::Left => {
                switch_branch(app, client, app.parent_branch()).await?;
                return Ok(());
            }
            KeyCode::Char('f') | KeyCode::Enter => {
                if let Some(id) = app.selected_message_id() {
                    if key.code == KeyCode::Char('f') || app.branches.contains(&id) {
                        switch_branch(app, client, id).await?;
                    }
                    return Ok(());
                }
            }
            _ => {}
        }
    }
    if key.code == KeyCode::Tab {
        app.toggle_focus();
        return Ok(());
    }
    if app.focus != Focus::Compose {
        handle_browse_key(app, key.code);
        return Ok(());
    }
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    match key.code {
        KeyCode::Char('j') if ctrl => app.input.insert_newline(),
        KeyCode::Enter if shift => app.input.insert_newline(),
        KeyCode::Enter => {
            let content = app.input.lines().join("\n");
            if !content.trim().is_empty() {
                client.send_message(&content).await?;
                app.take_input();
                report_typing(app, client).await?;
            }
        }
        KeyCode::PageUp => app.scroll_up(app.timeline_height),
        KeyCode::PageDown => app.scroll_down(app.timeline_height),
        KeyCode::Up if app.input.lines().len() == 1 => app.scroll_up(1),
        KeyCode::Down if app.input.lines().len() == 1 => app.scroll_down(1),
        _ => {
            app.input.input(key);
            report_typing(app, client).await?;
        }
    }
    Ok(())
}

async fn switch_branch(app: &mut App, client: &mut TimClient, branch_id: u64) -> Result<()> {
    client.send_typing_stopped().await?;
    app.switch_branch(branch_id);
    *client = client.in_branch(branch_id);
    report_typing(app, client).await
}

fn handle_browse_key(app: &mut App, key: KeyCode) {
    if app.focus == Focus::Inspect {
        match key {
            KeyCode::Char(tab @ '1'..='4') if app.selected_llm_call().is_some() => {
                app.inspector_tab = usize::from(tab as u8 - b'1');
                app.inspector_offset = 0;
            }
            KeyCode::Esc => app.focus = app.inspector_return,
            KeyCode::Up => app.inspector_offset = app.inspector_offset.saturating_sub(1),
            KeyCode::Down => app.inspector_offset = app.inspector_offset.saturating_add(1),
            KeyCode::PageUp => {
                app.inspector_offset = app.inspector_offset.saturating_sub(app.timeline_height)
            }
            KeyCode::PageDown => {
                app.inspector_offset = app.inspector_offset.saturating_add(app.timeline_height)
            }
            KeyCode::Home => app.inspector_offset = 0,
            KeyCode::End => app.inspector_offset = usize::MAX,
            _ => {}
        }
        return;
    }
    match key {
        KeyCode::Esc => app.focus = Focus::Compose,
        KeyCode::Up => app.select_item(false),
        KeyCode::Down => app.select_item(true),
        KeyCode::Enter => app.open_inspector(),
        _ => {}
    }
}

/// Tells the space when this timite starts or stops typing.
async fn report_typing(app: &mut App, client: &mut TimClient) -> Result<()> {
    match app.typing_transition() {
        Some(true) => client.send_typing_started().await?,
        Some(false) => client.send_typing_stopped().await?,
        None => {}
    }
    Ok(())
}
