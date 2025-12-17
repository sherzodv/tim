use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
    Frame,
};

use crate::app::{App, InputMode, TimelineItem};

const MAX_INPUT_HEIGHT: u16 = 10;

pub fn render(frame: &mut Frame, app: &App) {
    // Calculate input height based on content (min 3, max MAX_INPUT_HEIGHT)
    let input_lines = app.input_line_count() as u16;
    let input_height = (input_lines + 2).clamp(3, MAX_INPUT_HEIGHT); // +2 for borders

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(5),
            Constraint::Length(input_height),
        ])
        .split(frame.area());

    render_header(frame, app, chunks[0]);
    render_main(frame, app, chunks[1]);
    render_input(frame, app, chunks[2]);

    if app.show_help {
        render_help_popup(frame);
    }
}

fn render_header(frame: &mut Frame, app: &App, area: Rect) {
    let mode_str = match app.input_mode {
        InputMode::Normal => "NORMAL",
        InputMode::Insert => "INSERT",
    };

    let header = Paragraph::new(Line::from(vec![
        Span::styled(" Tim Terminal ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw(" | "),
        Span::styled(format!("@{}", app.my_nick), Style::default().fg(Color::Green)),
        Span::raw(" | "),
        Span::styled(format!("[{}]", mode_str), Style::default().fg(Color::Yellow)),
        Span::raw(" | "),
        Span::styled("[F1] Help  [q] Quit", Style::default().fg(Color::DarkGray)),
    ]));

    frame.render_widget(header, area);
}

fn render_main(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(30), Constraint::Length(25)])
        .split(area);

    render_timeline(frame, app, chunks[0]);
    render_sidebar(frame, app, chunks[1]);
}

fn render_timeline(frame: &mut Frame, app: &App, area: Rect) {
    // Build all lines for the timeline
    let lines: Vec<Line> = app
        .timeline
        .iter()
        .flat_map(|item| {
            match item {
                TimelineItem::Message { sender, content, timestamp } => {
                    let time = format_timestamp(*timestamp);
                    let prefix_len = format!("[{}] {}: ", time, sender).chars().count();

                    let msg_lines: Vec<Line> = content
                        .lines()
                        .enumerate()
                        .map(|(i, line_content)| {
                            if i == 0 {
                                Line::from(vec![
                                    Span::styled(format!("[{}] ", time), Style::default().fg(Color::DarkGray)),
                                    Span::styled(format!("{}: ", sender), Style::default().fg(Color::Cyan)),
                                    Span::raw(line_content),
                                ])
                            } else {
                                Line::from(vec![
                                    Span::raw(" ".repeat(prefix_len)),
                                    Span::raw(line_content),
                                ])
                            }
                        })
                        .collect();

                    if msg_lines.is_empty() {
                        vec![Line::from(vec![
                            Span::styled(format!("[{}] ", time), Style::default().fg(Color::DarkGray)),
                            Span::styled(format!("{}: ", sender), Style::default().fg(Color::Cyan)),
                        ])]
                    } else {
                        msg_lines
                    }
                }
                TimelineItem::TimiteConnected { nick, timestamp } => {
                    let time = format_timestamp(*timestamp);
                    vec![Line::from(vec![
                        Span::styled(format!("[{}] ", time), Style::default().fg(Color::DarkGray)),
                        Span::styled(format!("{} ", nick), Style::default().fg(Color::Green)),
                        Span::styled("joined", Style::default().fg(Color::Green)),
                    ])]
                }
                TimelineItem::TimiteDisconnected { nick, timestamp } => {
                    let time = format_timestamp(*timestamp);
                    vec![Line::from(vec![
                        Span::styled(format!("[{}] ", time), Style::default().fg(Color::DarkGray)),
                        Span::styled(format!("{} ", nick), Style::default().fg(Color::Red)),
                        Span::styled("left", Style::default().fg(Color::Red)),
                    ])]
                }
                TimelineItem::AbilityCall { caller, ability_name, timestamp } => {
                    let time = format_timestamp(*timestamp);
                    vec![Line::from(vec![
                        Span::styled(format!("[{}] ", time), Style::default().fg(Color::DarkGray)),
                        Span::styled(format!("{} ", caller), Style::default().fg(Color::Magenta)),
                        Span::raw("called "),
                        Span::styled(ability_name, Style::default().fg(Color::Yellow)),
                    ])]
                }
                TimelineItem::AbilityOutcome { ability_name, success, timestamp } => {
                    let time = format_timestamp(*timestamp);
                    let status_color = if *success { Color::Green } else { Color::Red };
                    let status_text = if *success { "completed" } else { "failed" };
                    vec![Line::from(vec![
                        Span::styled(format!("[{}] ", time), Style::default().fg(Color::DarkGray)),
                        Span::styled(ability_name, Style::default().fg(Color::Yellow)),
                        Span::raw(" "),
                        Span::styled(status_text, Style::default().fg(status_color)),
                    ])]
                }
            }
        })
        .collect();

    let total_lines = lines.len();

    // Calculate scroll to show end by default, but respect manual scroll
    let scroll_y = app.timeline_scroll.min(total_lines.saturating_sub(1));

    let timeline = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" Timeline ({}) ", total_lines)),
        )
        .scroll((scroll_y as u16, 0));

    frame.render_widget(timeline, area);
}

fn render_sidebar(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    // Online timites
    let timites: Vec<ListItem> = app
        .online_timites
        .values()
        .map(|t| {
            let style = if t.id == app.my_timite_id {
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if t.id == app.my_timite_id { "> " } else { "  " };
            ListItem::new(Line::from(Span::styled(
                format!("{}{}", prefix, t.nick),
                style,
            )))
        })
        .collect();

    let timites_list = List::new(timites)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Online "),
        );

    frame.render_widget(timites_list, chunks[0]);

    // Abilities
    let abilities: Vec<ListItem> = app
        .abilities
        .iter()
        .flat_map(|ta| {
            ta.abilities.iter().map(|a| {
                ListItem::new(Line::from(Span::styled(
                    format!("  /{}", a.name),
                    Style::default().fg(Color::Yellow),
                )))
            })
        })
        .collect();

    let abilities_list = List::new(abilities)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Abilities "),
        );

    frame.render_widget(abilities_list, chunks[1]);
}

fn render_input(frame: &mut Frame, app: &App, area: Rect) {
    let input_style = match app.input_mode {
        InputMode::Normal => Style::default(),
        InputMode::Insert => Style::default().fg(Color::Yellow),
    };

    let (cursor_line, cursor_col) = app.cursor_line_col();
    let inner_height = area.height.saturating_sub(2) as usize; // subtract borders
    let inner_width = area.width.saturating_sub(2) as usize;

    // Calculate scroll offset to keep cursor visible
    let scroll_y = if cursor_line >= inner_height {
        cursor_line - inner_height + 1
    } else {
        0
    };

    let scroll_x = if cursor_col >= inner_width {
        cursor_col - inner_width + 1
    } else {
        0
    };

    let input = Paragraph::new(app.input.as_str())
        .style(input_style)
        .scroll((scroll_y as u16, scroll_x as u16))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Message (i to type, Enter to send, Ctrl+J for new line) "),
        );

    frame.render_widget(input, area);

    if app.input_mode == InputMode::Insert {
        let visual_line = cursor_line.saturating_sub(scroll_y);
        let visual_col = cursor_col.saturating_sub(scroll_x);
        frame.set_cursor_position((
            area.x + visual_col as u16 + 1,
            area.y + visual_line as u16 + 1,
        ));
    }
}

fn render_help_popup(frame: &mut Frame) {
    let area = centered_rect(60, 70, frame.area());

    let help_text = vec![
        Line::from(Span::styled("Keybindings", Style::default().add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from(Span::styled("Normal Mode:", Style::default().fg(Color::Cyan))),
        Line::from("  i           Enter insert mode"),
        Line::from("  q/Ctrl+D    Quit"),
        Line::from("  j/k         Scroll down/up"),
        Line::from("  G           Scroll to bottom"),
        Line::from("  F1          Toggle help"),
        Line::from(""),
        Line::from(Span::styled("Insert Mode:", Style::default().fg(Color::Cyan))),
        Line::from("  Esc         Return to normal mode"),
        Line::from("  Enter       Send message"),
        Line::from("  Ctrl+J      New line"),
        Line::from("  Backspace   Delete character"),
        Line::from("  Arrows      Move cursor"),
        Line::from(""),
        Line::from(Span::styled("Press F1 or Esc to close", Style::default().fg(Color::DarkGray))),
    ];

    let help = Paragraph::new(help_text)
        .wrap(Wrap { trim: true })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Help ")
                .style(Style::default().bg(Color::DarkGray)),
        );

    frame.render_widget(Clear, area);
    frame.render_widget(help, area);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn format_timestamp(ts: u64) -> String {
    use chrono::{TimeZone, Utc};
    let dt = Utc.timestamp_millis_opt(ts as i64).single();
    match dt {
        Some(dt) => dt.format("%H:%M").to_string(),
        None => "--:--".to_string(),
    }
}
