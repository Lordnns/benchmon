//! Logs tab — scrollable timestamped log viewer.
//!
//! Scroll keys (same as terminal):
//!   PgUp / PgDn   10 lines
//!   ↑ / ↓          1 line
//!   Home           jump to oldest entry
//!   End            snap to newest (default)
//!   Mouse wheel    3 lines

use ratatui::{
    prelude::*,
    widgets::*,
};
use crate::app::{App, LogLevel};

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let total   = app.logs.len();
    let visible = area.height.saturating_sub(2) as usize; // subtract border

    // Build all lines first so we can apply the scroll window
    let all_lines: Vec<ListItem> = app.logs.iter().map(|entry| {
        let (icon, color) = match entry.level {
            LogLevel::Info    => ("ℹ", Color::Cyan),
            LogLevel::Warn    => ("⚠", Color::Yellow),
            LogLevel::Error   => ("✗", Color::Red),
            LogLevel::Success => ("✓", Color::Green),
        };
        ListItem::new(Line::from(vec![
            Span::styled(format!(" {} ", entry.timestamp),
                Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{icon} "),
                Style::default().fg(color).bold()),
            Span::styled(entry.message.clone(),
                Style::default().fg(color)),
        ]))
    }).collect();

    // scroll_offset: 0 = pinned to bottom (newest), >0 = scrolled up
    let max_scroll = total.saturating_sub(visible);
    let scroll_up  = app.log_scroll.min(max_scroll);
    let start      = total.saturating_sub(visible).saturating_sub(scroll_up);
    let end        = (start + visible).min(total);

    let visible_lines: Vec<ListItem> = all_lines[start..end].to_vec();

    // Scroll hint shown in bottom border
    let scroll_hint = if scroll_up > 0 {
        format!(" ↑{scroll_up} lines — End to snap bottom ")
    } else if total > visible {
        " (at bottom — PgUp/↑ to scroll) ".to_string()
    } else {
        String::new()
    };

    let list = List::new(visible_lines)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(Span::styled(
                format!(" Logs ({total} entries) "),
                Style::default().fg(Color::Cyan).bold()))
            .title_bottom(Span::styled(
                scroll_hint,
                Style::default().fg(Color::Yellow))));

    f.render_widget(list, area);
}