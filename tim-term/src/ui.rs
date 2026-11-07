use ratatui::{
    layout::{Constraint, Layout, Margin, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};
use textwrap::core::display_width;

use crate::app::{App, Focus, TimelineItem};
use crate::llm::LlmCall;

const MAX_INPUT_HEIGHT: u16 = 8;
/// How much of a tool call's arguments and outcome the timeline shows.
const TOOL_CALL_CHARS: usize = 48;
const PROMPT: &str = "› ";

pub fn render(frame: &mut Frame, app: &mut App) {
    let input_height = (app.input.lines().len() as u16).clamp(1, MAX_INPUT_HEIGHT);
    let [header, notice, timeline, rule, input, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(u16::from(app.notice.is_some())),
        Constraint::Min(3),
        Constraint::Length(1),
        Constraint::Length(input_height),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    render_header(frame, app, header);
    if let Some(message) = &app.notice {
        frame.render_widget(
            Paragraph::new(format!(" ! {message}")).fg(Color::Yellow),
            notice,
        );
    }
    if app.focus == Focus::Branches {
        render_branches(frame, app, timeline);
    } else if app.focus == Focus::Processes {
        render_processes(frame, app, timeline);
    } else if app.focus == Focus::Inspect {
        render_inspector(frame, app, timeline);
    } else {
        render_timeline(frame, app, timeline);
    }
    frame.render_widget(Paragraph::new("─".repeat(rule.width as usize)).dim(), rule);
    render_input(frame, app, input);
    let help = match app.focus {
        Focus::Compose => " Compose · F2 branches · F3 main · F4 processes · Tab browse · Enter send · Ctrl+J newline",
        Focus::Browse => " Browse · ↑/↓ select · Enter open · f fork · ← parent · b branches · F3 main · Tab compose",
        Focus::Processes => " Processes · ↑/↓ select · Enter logs · x stop group · Esc back · Tab compose",
        Focus::Branches => " Branches · ↑/↓ select · Enter open · Esc compose · F3 main",
        Focus::Inspect if app.selected_process().is_some() => " Process · x stop group · ↑/↓ PgUp/PgDn scroll · Esc back · Tab compose",
        Focus::Inspect if app.selected_llm_call().is_some() => " LLM · 1–4 tabs · ↑/↓ PgUp/PgDn scroll · Esc back · Tab compose",
        Focus::Inspect => " Inspect · ↑/↓ PgUp/PgDn scroll · Home/End · Esc back · Tab compose · Ctrl+C quit",
    };
    frame.render_widget(Paragraph::new(help).dim(), footer);
}

fn render_processes(frame: &mut Frame, app: &App, area: Rect) {
    let items = app.process_items();
    if items.is_empty() {
        frame.render_widget(
            Paragraph::new(
                " No processes in this branch. Use sh.start for a session background command.",
            )
            .dim(),
            area,
        );
        return;
    }
    let height = usize::from(area.height);
    let selected = items
        .iter()
        .position(|index| Some(*index) == app.selected_item)
        .unwrap_or(0);
    let skip = selected.saturating_sub(height.saturating_sub(1));
    let lines: Vec<Line> = items
        .into_iter()
        .skip(skip)
        .take(height)
        .map(|index| {
            let TimelineItem::Process(process) = &app.timeline[index] else {
                unreachable!()
            };
            let notice = if process.stop_message.is_empty() {
                String::new()
            } else {
                format!("{} · ", process.stop_message)
            };
            let line = Line::raw(format!(
                " {notice}{} · {}",
                process.label(),
                process.process_id.chars().take(8).collect::<String>()
            ));
            if app.selected_item == Some(index) {
                line.reversed()
            } else {
                line
            }
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), area);
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
    let running = app.timeline.iter().filter(|item| matches!(item,
        TimelineItem::Process(process) if process.branch_id == app.branch_id && process.status() == "running"
    )).count();
    if running > 0 {
        header.push(Span::styled(
            format!(" · {running} processes (F4)"),
            Style::new().fg(Color::Yellow),
        ));
    }
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
    let mut previous_author = None;
    for (index, item) in app.timeline.iter().enumerate() {
        let author = item_author(item, app);
        let mut rendered = Vec::new();
        if author != previous_author {
            if let Some(author) = &author {
                rendered.push(Line::from(Span::styled(
                    author.clone(),
                    Style::new().fg(Color::Cyan).bold(),
                )));
            }
        }
        previous_author = author;
        rendered.extend(item_lines(item, width));
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
                    "  ↳ branch #{event_id} · {status}{}",
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
    if let Some(process) = app.selected_process() {
        let text = process.details();
        render_inspector_text(frame, app, area.inner(Margin::new(1, 0)), &text);
        return;
    }
    if app.selected_llm_call().is_some() {
        render_llm_inspector(frame, app, area);
        return;
    }
    let Some(TimelineItem::TaskToolCall {
        username,
        tool,
        arguments,
        outcome,
        success,
    }) = app.selected_item.and_then(|index| app.timeline.get(index))
    else {
        return;
    };
    let area = area.inner(Margin::new(1, 0));
    let [header, body] = Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).areas(area);
    let status = if *success { "returned" } else { "failed" };
    let color = if *success { Color::Cyan } else { Color::Red };
    frame.render_widget(
        Paragraph::new(format!("{username} · {tool} · {status}"))
            .fg(color)
            .bold(),
        header,
    );

    let text = format!(
        "Arguments\n{arguments}\n\n{} (as recorded)\n{outcome}",
        if *success { "Output" } else { "Error" }
    );
    render_inspector_text(frame, app, body, &text);
}

fn render_llm_inspector(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(call) = app.selected_llm_call() else {
        return;
    };
    let area = area.inner(Margin::new(1, 0));
    let [header, tabs, body] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Min(0),
    ])
    .areas(area);
    frame.render_widget(
        Paragraph::new(call.label()).fg(llm_color(call)).bold(),
        header,
    );
    let names = ["1 Summary", "2 Request", "3 Response", "4 Events"];
    let spans: Vec<Span> = names
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let style = if index == app.inspector_tab {
                Style::new().reversed()
            } else {
                Style::new().dim()
            };
            Span::styled(format!(" {name} "), style)
        })
        .collect();
    frame.render_widget(Paragraph::new(Line::from(spans)), tabs);
    let text = call.details(app.inspector_tab);
    render_inspector_text(frame, app, body, &text);
}

fn render_inspector_text(frame: &mut Frame, app: &mut App, body: Rect, text: &str) {
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

fn item_author(item: &TimelineItem, app: &App) -> Option<String> {
    let author = match item {
        TimelineItem::Message { sender, .. } => sender,
        TimelineItem::TimiteConnected { username, .. }
        | TimelineItem::TimiteDisconnected { username, .. }
        | TimelineItem::TaskStarted { username, .. }
        | TimelineItem::TaskFinished { username, .. }
        | TimelineItem::TaskToolCall { username, .. } => username,
        TimelineItem::AbilityCall { caller, .. } => caller,
        TimelineItem::Process(process) => {
            return Some(
                app.usernames
                    .get(&process.timite_id)
                    .cloned()
                    .unwrap_or_else(|| format!("user-{}", process.timite_id)),
            )
        }
        TimelineItem::LlmCall(call) => {
            return Some(
                app.usernames
                    .get(&call.timite_id)
                    .cloned()
                    .unwrap_or_else(|| format!("user-{}", call.timite_id)),
            )
        }
        TimelineItem::AbilityOutcome { .. } => return None,
    };
    Some(author.clone())
}

fn item_lines(item: &TimelineItem, width: usize) -> Vec<Line<'static>> {
    let (marker, text, style) = match item {
        TimelineItem::Process(process) => {
            let color = match process.status() {
                "succeeded" => Color::Green,
                "failed" | "timed out" => Color::Red,
                "running" => Color::Cyan,
                _ => Color::Yellow,
            };
            (
                "·",
                format!("sh · {} · Enter inspect", process.label()),
                Style::new().fg(color),
            )
        }
        TimelineItem::LlmCall(call) => {
            let marker = match call.status() {
                "succeeded" => "✓",
                "failed" => "✗",
                "running" => "◌",
                _ => "?",
            };
            (marker, call.label(), Style::new().fg(llm_color(call)))
        }
        TimelineItem::Message { content, .. } => ("›", content.clone(), Style::new()),
        TimelineItem::TimiteConnected { .. } => ("·", "joined".into(), Style::new().dim()),
        TimelineItem::TimiteDisconnected { .. } => ("·", "left".into(), Style::new().dim()),
        TimelineItem::AbilityCall { ability_name, .. } => {
            ("·", format!("called {ability_name}"), Style::new().dim())
        }
        TimelineItem::AbilityOutcome {
            ability_name,
            success,
            ..
        } => {
            let status = if *success { "completed" } else { "failed" };
            ("·", format!("{ability_name} {status}"), Style::new().dim())
        }
        TimelineItem::TaskStarted { task, .. } => {
            ("·", format!("started: {task}"), Style::new().dim())
        }
        TimelineItem::TaskFinished {
            task,
            error,
            duration_ms,
            ..
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
            (
                "·",
                format!("{status}: {task}{duration}{error}"),
                Style::new().dim(),
            )
        }
        TimelineItem::TaskToolCall {
            tool,
            arguments,
            outcome,
            success,
            ..
        } => {
            let status = if *success { "" } else { "error: " };
            (
                "·",
                format!(
                    "{tool} {} → {status}{}",
                    brief(arguments, TOOL_CALL_CHARS),
                    brief(outcome, TOOL_CALL_CHARS)
                ),
                Style::new().dim(),
            )
        }
    };
    message_lines(marker, &text, width, style)
}

fn llm_color(call: &LlmCall) -> Color {
    match call.status() {
        "failed" => Color::Red,
        "succeeded" => Color::Green,
        "running" => Color::Cyan,
        _ => Color::Yellow,
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

/// Each event keeps its own marker; wrapped lines align with the message body.
fn message_lines(marker: &str, content: &str, width: usize, style: Style) -> Vec<Line<'static>> {
    let prefix = format!("  {marker} ");
    let head_width = display_width(&prefix);
    let indent = " ".repeat(head_width);
    let body_width = width.saturating_sub(head_width).max(1);
    let mut lines = Vec::new();
    for source in content.split('\n') {
        for piece in textwrap::wrap(source, body_width) {
            let prefix = if lines.is_empty() {
                prefix.clone()
            } else {
                indent.clone()
            };
            lines.push(Line::from(vec![
                Span::styled(prefix, style),
                Span::styled(piece.into_owned(), style),
            ]));
        }
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(prefix, style)));
    }
    lines
}
