use chrono::{Local, TimeZone};
use ratatui::{
    layout::{Constraint, Layout, Margin, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};
use textwrap::core::display_width;

use crate::app::{App, TimelineItem};

const MAX_INPUT_HEIGHT: u16 = 8;
/// How much of a tool call's arguments and outcome the timeline shows.
const TOOL_CALL_CHARS: usize = 48;
const MIN_BODY_WIDTH: usize = 16;
const PROMPT: &str = "› ";

pub fn render(frame: &mut Frame, app: &mut App) {
    let input_height = (app.input.lines().len() as u16).clamp(1, MAX_INPUT_HEIGHT);
    let [header, timeline, rule, input, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(1),
        Constraint::Length(input_height),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    render_header(frame, app, header);
    render_timeline(frame, app, timeline);
    frame.render_widget(Paragraph::new("─".repeat(rule.width as usize)).dim(), rule);
    render_input(frame, app, input);
    frame.render_widget(
        Paragraph::new(" Enter send · Ctrl+J newline · ↑/↓ PgUp/PgDn scroll · Ctrl+C quit").dim(),
        footer,
    );
}

fn render_header(frame: &mut Frame, app: &App, area: Rect) {
    let mut online: Vec<&str> = app
        .online_timites
        .values()
        .map(|timite| timite.username.as_str())
        .collect();
    online.sort_unstable();

    let mut header = vec![
        Span::styled(" tim", Style::new().fg(Color::Cyan).bold()),
        Span::styled(" · ", Style::new().dim()),
        Span::styled(
            format!("@{}", app.my_username),
            Style::new().fg(Color::Green),
        ),
        Span::styled(
            format!(" · online: {}", online.join(", ")),
            Style::new().dim(),
        ),
    ];
    let status = app.status();
    if !status.is_empty() {
        header.push(Span::styled(
            format!(" · {}", status.join(", ")),
            Style::new().fg(Color::Yellow).italic(),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(header)), area);
}

/// Pre-wrapped lines let the scroll offset count from the bottom in visual lines.
fn render_timeline(frame: &mut Frame, app: &mut App, area: Rect) {
    let area = area.inner(Margin::new(1, 0));
    let width = area.width as usize;
    let lines: Vec<Line> = app
        .timeline
        .iter()
        .flat_map(|item| item_lines(item, width))
        .collect();

    let height = area.height as usize;
    let max_offset = lines.len().saturating_sub(height);
    app.scroll_offset = app.scroll_offset.min(max_offset);
    app.timeline_height = height;
    let skip = max_offset - app.scroll_offset;

    frame.render_widget(Paragraph::new(lines).scroll((skip as u16, 0)), area);
}

fn render_input(frame: &mut Frame, app: &App, area: Rect) {
    let [prompt, text] = Layout::horizontal([
        Constraint::Length(display_width(PROMPT) as u16),
        Constraint::Min(1),
    ])
    .areas(area);
    frame.render_widget(Paragraph::new(PROMPT).fg(Color::Cyan), prompt);
    frame.render_widget(&app.input, text);
}

fn item_lines(item: &TimelineItem, width: usize) -> Vec<Line<'static>> {
    match item {
        TimelineItem::Message {
            sender,
            content,
            timestamp,
        } => message_lines(format_time(*timestamp), sender, content, width),
        TimelineItem::TimiteConnected {
            username,
            timestamp,
        } => {
            vec![system_line(
                format_time(*timestamp),
                format!("{username} joined"),
            )]
        }
        TimelineItem::TimiteDisconnected {
            username,
            timestamp,
        } => {
            vec![system_line(
                format_time(*timestamp),
                format!("{username} left"),
            )]
        }
        TimelineItem::AbilityCall {
            caller,
            ability_name,
            timestamp,
        } => vec![system_line(
            format_time(*timestamp),
            format!("{caller} called {ability_name}"),
        )],
        TimelineItem::AbilityOutcome {
            ability_name,
            success,
            timestamp,
        } => {
            let status = if *success { "completed" } else { "failed" };
            vec![system_line(
                format_time(*timestamp),
                format!("{ability_name} {status}"),
            )]
        }
        TimelineItem::TaskStarted {
            username,
            task,
            timestamp,
        } => vec![system_line(
            format_time(*timestamp),
            format!("{username} started: {task}"),
        )],
        TimelineItem::TaskFinished {
            username,
            task,
            timestamp,
        } => vec![system_line(
            format_time(*timestamp),
            format!("{username} finished: {task}"),
        )],
        TimelineItem::TaskToolCall {
            username,
            tool,
            arguments,
            outcome,
            success,
            timestamp,
        } => {
            let status = if *success { "" } else { "error: " };
            vec![system_line(
                format_time(*timestamp),
                format!(
                    "{username} {tool} {} → {status}{}",
                    brief(arguments, TOOL_CALL_CHARS),
                    brief(outcome, TOOL_CALL_CHARS)
                ),
            )]
        }
    }
}

/// The first line of the text, cut to `max` characters.
fn brief(text: &str, max: usize) -> String {
    let line = text.lines().next().unwrap_or("").trim();
    let mut brief: String = line.chars().take(max).collect();
    if line.chars().count() > max {
        brief.push('…');
    }
    brief
}

fn system_line(time: String, text: String) -> Line<'static> {
    Line::from(vec![
        Span::styled(time, Style::new().dim()),
        Span::styled(format!(" · {text}"), Style::new().dim()),
    ])
}

/// Wraps the message under its `time sender:` head; continuation lines hang below the text.
fn message_lines(time: String, sender: &str, content: &str, width: usize) -> Vec<Line<'static>> {
    let head_width = display_width(&time) + display_width(sender) + 3;
    let indent = " ".repeat(head_width);
    let body_width = width.saturating_sub(head_width).max(MIN_BODY_WIDTH);
    let head = vec![
        Span::styled(time, Style::new().dim()),
        Span::raw(" "),
        Span::styled(format!("{sender}:"), Style::new().fg(Color::Cyan)),
        Span::raw(" "),
    ];

    let mut lines = Vec::new();
    for source in content.lines() {
        for piece in textwrap::wrap(source, body_width) {
            let mut spans = if lines.is_empty() {
                head.clone()
            } else {
                vec![Span::raw(indent.clone())]
            };
            spans.push(Span::raw(piece.into_owned()));
            lines.push(Line::from(spans));
        }
    }
    if lines.is_empty() {
        lines.push(Line::from(head));
    }
    lines
}

fn format_time(ts: u64) -> String {
    match Local.timestamp_millis_opt(ts as i64).single() {
        Some(dt) => dt.format("%H:%M").to_string(),
        None => "--:--".to_string(),
    }
}
