mod app;
mod client;
mod error;
mod event;
mod ui;

use std::io;
use std::time::Duration;

use crossterm::{
    event::{
        DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        KeyCode, KeyModifiers,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures::StreamExt;
use ratatui::{backend::CrosstermBackend, Terminal};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::app::{App, InputMode};
use crate::client::{ClientConfig, TimClient};
use crate::error::Result;
use crate::event::{AppEvent, EventHandler};

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")))
        .with(tracing_subscriber::fmt::layer().with_writer(io::stderr))
        .init();

    let endpoint = std::env::var("TIM_ENDPOINT").unwrap_or_else(|_| "http://127.0.0.1:8787".into());
    let nick = std::env::var("TIM_NICK").unwrap_or_else(|_| whoami::username());

    let config = ClientConfig {
        endpoint,
        nick: nick.clone(),
        timite_id: None,
    };

    tracing::info!("Connecting to Tim server...");
    let mut client = TimClient::connect(config).await?;
    let timite_id = client.timite_id();

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste
    )?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(timite_id, nick);

    // Load initial abilities
    if let Ok(abilities) = client.list_abilities().await {
        app.set_abilities(abilities);
    }

    // Load timeline history
    if let Ok(res) = client.get_timeline(0, 100).await {
        for timite in &res.timites {
            app.add_timite_to_cache(timite);
        }
        for event in res.events {
            app.handle_space_event(event);
        }
        app.scroll_to_bottom();
    }

    // Subscribe to space events
    let mut space_stream = client.subscribe_to_space().await?;

    let mut events = EventHandler::new(Duration::from_millis(250));
    let event_tx = events.sender();

    // Spawn task to forward space events
    tokio::spawn(async move {
        while let Some(Ok(event)) = space_stream.next().await {
            if event_tx.send(AppEvent::Space(event)).is_err() {
                break;
            }
        }
    });

    let result = run_app(&mut terminal, &mut app, &mut events, &mut client).await;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        DisableBracketedPaste
    )?;
    terminal.show_cursor()?;

    result
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
                handle_key(app, client, key.code, key.modifiers).await?;
            }
            AppEvent::Paste(text) => {
                if app.input_mode == InputMode::Insert {
                    app.paste(&text);
                }
            }
            AppEvent::Tick => {}
            AppEvent::Space(event) => {
                app.handle_space_event(event);
                app.scroll_to_bottom();
            }
        }
    }

    Ok(())
}

async fn handle_key(
    app: &mut App,
    client: &mut TimClient,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> Result<()> {
    // Global keybindings
    if code == KeyCode::F(1) {
        app.toggle_help();
        return Ok(());
    }

    if app.show_help {
        if matches!(code, KeyCode::Esc | KeyCode::F(1)) {
            app.show_help = false;
        }
        return Ok(());
    }

    match app.input_mode {
        InputMode::Normal => match code {
            KeyCode::Char('q') => app.quit(),
            KeyCode::Char('i') => app.enter_insert_mode(),
            KeyCode::Char('j') | KeyCode::Down => app.scroll_down(),
            KeyCode::Char('k') | KeyCode::Up => app.scroll_up(),
            KeyCode::Char('G') => app.scroll_to_bottom(),
            KeyCode::Char('c') | KeyCode::Char('d') if modifiers.contains(KeyModifiers::CONTROL) => app.quit(),
            _ => {}
        },
        InputMode::Insert => match code {
            KeyCode::Esc => app.enter_normal_mode(),
            // Ctrl+J for new line
            KeyCode::Char('j') if modifiers.contains(KeyModifiers::CONTROL) => app.enter_char('\n'),
            KeyCode::Enter => {
                let content = app.take_input();
                if !content.trim().is_empty() {
                    client.send_message(&content).await?;
                }
            }
            // Handle backspace - some terminals send Ctrl+H
            KeyCode::Backspace | KeyCode::Delete => app.delete_char(),
            KeyCode::Char('h') if modifiers.contains(KeyModifiers::CONTROL) => app.delete_char(),
            KeyCode::Left => app.move_cursor_left(),
            KeyCode::Right => app.move_cursor_right(),
            KeyCode::Up => app.move_cursor_up(),
            KeyCode::Down => app.move_cursor_down(),
            KeyCode::Char('c') | KeyCode::Char('d') if modifiers.contains(KeyModifiers::CONTROL) => app.quit(),
            // Handle carriage return as newline (for terminals that send \r when pasting)
            KeyCode::Char('\r') => app.enter_char('\n'),
            KeyCode::Char(c) => app.enter_char(c),
            _ => {}
        },
    }

    Ok(())
}
