//! Terminal tab — embedded shell with scrollable output and command history.

use ratatui::{
    prelude::*,
    widgets::*,
};
use crate::app::App;

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(3),
        ])
        .split(area);

    let visible = chunks[0].height.saturating_sub(2) as usize;
    let total   = app.terminal_output.len();

    let max_scroll = total.saturating_sub(visible);
    let scroll_up  = app.terminal_scroll.min(max_scroll);
    let start      = total.saturating_sub(visible).saturating_sub(scroll_up);
    let end        = (start + visible).min(total);

    let output_lines: Vec<Line> = app.terminal_output[start..end]
        .iter()
        .map(|line| {
            if line.starts_with("$ ") {
                Line::from(Span::styled(
                    format!(" {line}"),
                    Style::default().fg(Color::Green).bold()))
            } else if line.starts_with("ERR:") {
                Line::from(Span::styled(
                    format!(" {line}"),
                    Style::default().fg(Color::Red)))
            } else if line.starts_with("WARNING") {
                Line::from(Span::styled(
                    format!(" {line}"),
                    Style::default().fg(Color::Yellow)))
            } else {
                Line::from(Span::styled(
                    format!(" {line}"),
                    Style::default().fg(Color::White)))
            }
        })
        .collect();

    // Bottom title: scroll position hint
    let scroll_hint = if scroll_up > 0 {
        format!(" ↑{scroll_up} lines — PgDn/End to snap bottom ")
    } else {
        String::new()
    };

    let border_style = if app.terminal_focused {
        Style::default().fg(Color::Green)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let output_block = Paragraph::new(output_lines)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(Span::styled(
                format!(" Terminal Output ({total} lines) "),
                Style::default().fg(Color::Cyan).bold()))
            .title_bottom(Span::styled(
                scroll_hint,
                Style::default().fg(Color::Yellow))));

    f.render_widget(output_block, chunks[0]);

    // ---- Input bar ----
    let cursor_char = if app.tick_count % 10 < 5 { "█" } else { " " };
    let input_text  = format!(" $ {}{}", app.terminal_input, cursor_char);

    let input_style = if app.terminal_focused {
        Style::default().fg(Color::Green)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    // Show history position when browsing
    let history_hint = match app.cmd_history_idx {
        Some(i) => format!(
            " history {}/{} ",
            i + 1,
            app.cmd_history.len()
        ),
        None if !app.cmd_history.is_empty() =>
            format!(" {} cmds ", app.cmd_history.len()),
        _ => String::new(),
    };

    let input_title = if app.terminal_focused {
        format!(" Input (Esc=unfocus  ↑↓=history){history_hint}")
    } else {
        " Input (Enter=focus) ".to_string()
    };

    let input_block = Paragraph::new(Line::from(Span::styled(
        input_text, input_style.bold())))
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(Span::styled(input_title,
                Style::default().fg(Color::Cyan).bold())));

    f.render_widget(input_block, chunks[1]);
}