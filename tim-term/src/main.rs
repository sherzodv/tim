mod app;
mod error;
mod event;
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

use crate::app::App;
use crate::error::Result;
use crate::event::{AppEvent, EventHandler};

/// Mouse stays with the terminal so text can be selected; the wheel arrives as arrow keys.
const ENABLE_ALTERNATE_SCROLL: &str = "\x1b[?1007h";
const DISABLE_ALTERNATE_SCROLL: &str = "\x1b[?1007l";

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")))
        .with(tracing_subscriber::fmt::layer().with_writer(io::stderr))
        .init();

    let endpoint = std::env::var("TIM_ENDPOINT").unwrap_or_else(|_| "http://127.0.0.1:8787".into());
    let nick = std::env::var("TIM_NICK").unwrap_or_else(|_| whoami::username());

    let config = TimClientConf {
        endpoint,
        nick: nick.clone(),
        provider: "tim-term".to_string(),
        timite_id: None,
    };

    tracing::info!("Connecting to Tim server...");
    let mut client = TimClient::new(config).await?;
    let timite_id = client.timite_id();

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

    let mut app = App::new(timite_id, nick);

    // Load timeline history
    let mut cursor = None;
    if let Ok(res) = client.get_timeline(0, 100).await {
        for timite in &res.timites {
            app.add_timite_to_cache(timite);
        }
        cursor = res
            .events
            .iter()
            .filter_map(|event| event.metadata.as_ref())
            .map(|metadata| metadata.id)
            .max();
        for event in res.events {
            app.handle_space_event(event);
        }
    }

    let mut events = EventHandler::new();
    let forwarder = SpaceForwarder {
        tx: events.sender(),
    };
    let mut runner = AgentRunner::new(&client).await;
    tokio::spawn(async move {
        if let Err(error) = runner.start(forwarder, cursor).await {
            tracing::error!(%error, "space subscription ended");
        }
    });

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
            AppEvent::Key(key) => handle_key(app, client, key).await?,
            AppEvent::Paste(text) => app.paste(&text),
            AppEvent::Resize => {}
            AppEvent::Space(event) => app.handle_space_event(event),
        }
    }

    Ok(())
}

async fn handle_key(app: &mut App, client: &mut TimClient, key: KeyEvent) -> Result<()> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    match key.code {
        KeyCode::Char('c') | KeyCode::Char('d') if ctrl => app.quit(),
        KeyCode::Char('j') if ctrl => app.input.insert_newline(),
        KeyCode::Enter if shift => app.input.insert_newline(),
        KeyCode::Enter => {
            let content = app.take_input();
            if !content.trim().is_empty() {
                client.send_message(&content).await?;
            }
        }
        KeyCode::PageUp => app.scroll_up(app.timeline_height),
        KeyCode::PageDown => app.scroll_down(app.timeline_height),
        KeyCode::Up if app.input.lines().len() == 1 => app.scroll_up(1),
        KeyCode::Down if app.input.lines().len() == 1 => app.scroll_down(1),
        _ => {
            app.input.input(key);
        }
    }
    Ok(())
}
