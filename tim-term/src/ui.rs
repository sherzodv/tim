use chrono::{Local, TimeZone};
use ratatui::{
    layout::{Constraint, Layout, Margin, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};
use textwrap::core::display_width;

use crate::app::{App, Focus, TimelineItem};

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
    if app.focus == Focus::Branches {
        render_branches(frame, app, timeline);
    } else if app.focus == Focus::Inspect {
        render_inspector(frame, app, timeline);
    } else {
        render_timeline(frame, app, timeline);
    }
    frame.render_widget(Paragraph::new("─".repeat(rule.width as usize)).dim(), rule);
    render_input(frame, app, input);
    let help = match app.focus {
        Focus::Compose => " Compose · F2 branches · F3 main · Tab browse · Enter send · Ctrl+J newline",
        Focus::Browse => " Browse · ↑/↓ select · Enter open · f fork · ← parent · b branches · F3 main · Tab compose",
        Focus::Branches => " Branches · ↑/↓ select · Enter open · Esc compose · F3 main",
        Focus::Inspect => " Inspect · ↑/↓ PgUp/PgDn scroll · Home/End · Esc back · Tab compose · Ctrl+C quit",
    };
    frame.render_widget(Paragraph::new(help).dim(), footer);
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
        Span::styled(
            format!(" / {} · ", app.branch_label(app.branch_id)),
            Style::new().fg(Color::Yellow),
        ),
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

fn render_branches(frame: &mut Frame, app: &App, area: Rect) {
    let height = usize::from(area.height);
    let skip = app.selected_branch.saturating_sub(height.saturating_sub(1));
    let lines: Vec<Line> = app
        .branches
        .iter()
        .enumerate()
        .skip(skip)
        .take(height)
        .map(|(index, id)| {
            let marker = if *id == app.branch_id { "●" } else { " " };
            let line = Line::raw(format!(
                " {marker} {}  {}",
                app.branch_label(*id),
                app.branch_status(*id)
            ));
            if index == app.selected_branch {
                line.reversed()
            } else {
                line
            }
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), area);
}

/// Pre-wrapped lines let the scroll offset count from the bottom in visual lines.
fn render_timeline(frame: &mut Frame, app: &mut App, area: Rect) {
    let area = area.inner(Margin::new(1, 0));
    let width = area.width as usize;
    let mut lines = Vec::new();
    let mut selected_line = None;
    for (index, item) in app.timeline.iter().enumerate() {
        let mut rendered = item_lines(item, width);
        let event_id = app.timeline_event_ids[index];
        if matches!(item, TimelineItem::Message { .. }) && app.branches.contains(&event_id) {
            let status = app.branch_status(event_id);
            let status = if status.is_empty() {
                String::new()
            } else {
                format!("{status} · ")
            };
            rendered.push(Line::from(Span::styled(
                format!(
                    "       ↳ branch #{event_id} · {status}{}",
                    if event_id == app.branch_id {
                        "current"
                    } else {
                        "Enter open"
                    }
                ),
                Style::new().fg(Color::Yellow),
            )));
        }
        if app.focus == Focus::Browse && app.selected_item == Some(index) {
            selected_line = Some(lines.len());
            for line in &mut rendered {
                line.style = Style::new().reversed();
            }
        }
        lines.extend(rendered);
    }

    let height = area.height as usize;
    let max_offset = lines.len().saturating_sub(height);
    app.scroll_offset = app.scroll_offset.min(max_offset);
    app.timeline_height = height;
    let mut skip = max_offset - app.scroll_offset;
    if let Some(selected) = selected_line {
        if selected < skip {
            skip = selected;
        } else if selected >= skip + height {
            skip = selected.saturating_sub(height.saturating_sub(1));
        }
        app.scroll_offset = max_offset.saturating_sub(skip);
    }

    frame.render_widget(
        Paragraph::new(
            lines
                .into_iter()
                .skip(skip)
                .take(height)
                .collect::<Vec<_>>(),
        ),
        area,
    );
}

/// Shows all recorded text; the sender may already have truncated the output.
fn render_inspector(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(TimelineItem::TaskToolCall {
        username,
        tool,
        arguments,
        outcome,
        success,
        timestamp,
    }) = app.selected_item.and_then(|index| app.timeline.get(index))
    else {
        return;
    };
    let area = area.inner(Margin::new(1, 0));
    let [header, body] = Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).areas(area);
    let status = if *success { "returned" } else { "failed" };
    let color = if *success { Color::Cyan } else { Color::Red };
    frame.render_widget(
        Paragraph::new(format!(
            "{} · {username} · {tool} · {status}",
            format_time(*timestamp)
        ))
        .fg(color)
        .bold(),
        header,
    );

    let text = format!(
        "Arguments\n{arguments}\n\n{} (as recorded)\n{outcome}",
        if *success { "Output" } else { "Error" }
    );
    let lines: Vec<Line> = text
        .split('\n')
        .flat_map(|line| {
            textwrap::wrap(line, usize::from(body.width).max(1))
                .into_iter()
                .map(|piece| Line::raw(piece.into_owned()))
        })
        .collect();
    let height = usize::from(body.height);
    app.timeline_height = height;
    app.inspector_offset = app.inspector_offset.min(lines.len().saturating_sub(height));
    frame.render_widget(
        Paragraph::new(
            lines
                .into_iter()
                .skip(app.inspector_offset)
                .take(height)
                .collect::<Vec<_>>(),
        ),
        body,
    );
}

fn render_input(frame: &mut Frame, app: &App, area: Rect) {
    let [prompt, text] = Layout::horizontal([
        Constraint::Length(display_width(PROMPT) as u16),
        Constraint::Min(1),
    ])
    .areas(area);
    frame.render_widget(Paragraph::new(PROMPT).fg(Color::Cyan), prompt);
    if app.focus == Focus::Compose {
        frame.render_widget(&app.input, text);
    } else {
        frame.render_widget(Paragraph::new(app.input.lines().join("\n")).dim(), text);
    }
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
            error,
            duration_ms,
            timestamp,
        } => {
            let status = if error.is_some() {
                "failed"
            } else {
                "finished"
            };
            let duration = duration_ms
                .map(|ms| format!(" ({ms} ms)"))
                .unwrap_or_default();
            let error = error
                .as_ref()
                .map(|error| format!(": {error}"))
                .unwrap_or_default();
            vec![system_line(
                format_time(*timestamp),
                format!("{username} {status}: {task}{duration}{error}"),
            )]
        }
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
