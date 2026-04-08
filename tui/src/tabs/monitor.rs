//! Monitor tab — live sparklines with per-section scrolling.
//!
//! ← / → on the Monitor tab cycles the focused section (CPU · Net · Disk).
//! ↑ / ↓ / PgUp / PgDn scroll within the focused section.
//! Mouse wheel scrolls the focused section.
//! Each section's border turns Cyan when it is focused.

use ratatui::{
    prelude::*,
    widgets::*,
};
use crate::app::{App, MonitorSection};

fn fmt_bytes(b: u64) -> String {
    if b >= 1_073_741_824 { format!("{:.1} GB", b as f64 / 1_073_741_824.0) }
    else if b >= 1_048_576 { format!("{:.1} MB", b as f64 / 1_048_576.0) }
    else if b >= 1024      { format!("{:.1} KB", b as f64 / 1024.0) }
    else                   { format!("{b} B") }
}

/// Focused-section border colour helper.
fn section_border(app: &App, section: MonitorSection) -> Style {
    if app.monitor_section == section {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

/// Scroll hint for a section.
fn scroll_hint(scroll: usize, total: usize, visible: usize) -> String {
    if total <= visible {
        return String::new();
    }
    let max = total.saturating_sub(visible);
    if scroll == 0 {
        format!(" ({total} items — ↑/PgUp to scroll) ")
    } else if scroll >= max {
        format!(" (top — ↓/PgDn to scroll down) ")
    } else {
        format!(" ↑{scroll}/{max} — ↑↓/PgUp/PgDn ")
    }
}

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(35), // CPU
            Constraint::Percentage(20), // Memory
            Constraint::Percentage(25), // Network
            Constraint::Percentage(20), // Disk + latency
        ])
        .split(area);

    render_cpu(f, chunks[0], app);
    render_memory(f, chunks[1], app);
    render_network(f, chunks[2], app);
    render_disk_and_latency(f, chunks[3], app);
}

// ------------------------------------------------------------------ //
//  CPU  — dynamic multi-column sparkline grid                        //
// ------------------------------------------------------------------ //
fn render_cpu(f: &mut Frame, area: Rect, app: &App) {
    let snap = match &app.latest_snapshot {
        Some(s) => s,
        None => {
            let p = Paragraph::new("  No CPU data — monitor not initialized")
                .block(Block::default().borders(Borders::ALL)
                    .border_style(section_border(app, MonitorSection::Cpu))
                    .title(cpu_title(app, 0)));
            f.render_widget(p, area);
            return;
        }
    };

    let core_count = snap.cpu.len();
    let row_h = 2u16; // lines per core sparkline

    // How many rows fit vertically inside the border
    let inner_h = area.height.saturating_sub(2) as usize;
    let rows_per_col = (inner_h / row_h as usize).max(1);

    // How many columns fit horizontally — minimum 22 chars per column
    let inner_w = area.width.saturating_sub(2) as usize;
    let col_min_w = 22usize;
    let num_cols  = (inner_w / col_min_w).max(1);

    // Total visible cores across all columns
    let visible_total = rows_per_col * num_cols;

    // Scroll is in units of "one full column-page"
    let max_scroll = core_count.saturating_sub(visible_total);
    let scroll     = app.monitor_cpu_scroll.min(max_scroll);
    let start      = scroll;
    let end        = (start + visible_total).min(core_count);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(section_border(app, MonitorSection::Cpu))
        .title(cpu_title(app, core_count))
        .title_bottom(Span::styled(
            scroll_hint(scroll, core_count, visible_total),
            Style::default().fg(Color::Yellow)));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if core_count == 0 { return; }

    // Split inner area into columns
    let col_constraints: Vec<Constraint> = (0..num_cols)
        .map(|_| Constraint::Ratio(1, num_cols as u32))
        .collect();
    let col_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(col_constraints)
        .split(inner);

    // Each column gets rows_per_col rows of height row_h
    let row_constraints: Vec<Constraint> = (0..rows_per_col)
        .map(|_| Constraint::Length(row_h))
        .collect();

    for col in 0..num_cols {
        if col_chunks.len() <= col { break; }
        let col_area = col_chunks[col];

        let row_areas = Layout::default()
            .direction(Direction::Vertical)
            .constraints(row_constraints.clone())
            .split(col_area);

        for row in 0..rows_per_col {
            let core_idx = start + col * rows_per_col + row;
            if core_idx >= end || row >= row_areas.len() { break; }

            let cpu   = &snap.cpu[core_idx];
            let label = format!(" C{:<2} {:>4}MHz", cpu.core_id, cpu.freq_mhz);

            let sparkline_data: Vec<u64> = app.cpu_history.get(core_idx)
                .map(|h| h.as_slice().iter().map(|v| (*v * 100.0) as u64).collect())
                .unwrap_or_default();

            let spark = Sparkline::default()
                .data(&sparkline_data)
                .max(10000)
                .style(Style::default().fg(Color::Cyan))
                .block(Block::default()
                    .title(Span::styled(label,
                        Style::default().fg(Color::White))));

            f.render_widget(spark, row_areas[row]);
        }
    }
}

fn cpu_title<'a>(app: &App, total: usize) -> Span<'a> {
    let focused = app.monitor_section == MonitorSection::Cpu;
    let style   = if focused {
        Style::default().fg(Color::Cyan).bold()
    } else {
        Style::default().fg(Color::White).bold()
    };
    let hint = if focused { " [←→ switch  ↑↓ scroll] " } else { "" };
    Span::styled(format!(" CPU Cores ({total}) {hint}"), style)
}

// ------------------------------------------------------------------ //
//  Memory                                                             //
// ------------------------------------------------------------------ //
fn render_memory(f: &mut Frame, area: Rect, app: &App) {
    let snap = match &app.latest_snapshot {
        Some(s) => s,
        None => {
            let p = Paragraph::new("  No memory data")
                .block(Block::default().borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::DarkGray))
                    .title(Span::styled(" Memory ", Style::default().fg(Color::Magenta).bold())));
            f.render_widget(p, area);
            return;
        }
    };

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(area);

    let m   = &snap.mem;
    let pct = if m.total_bytes > 0 {
        m.used_bytes as f64 / m.total_bytes as f64 * 100.0
    } else { 0.0 };

    let info = vec![
        Line::from(vec![
            Span::styled("  Used:  ", Style::default().fg(Color::DarkGray)),
            Span::raw(fmt_bytes(m.used_bytes)),
        ]),
        Line::from(vec![
            Span::styled("  Free:  ", Style::default().fg(Color::DarkGray)),
            Span::raw(fmt_bytes(m.available_bytes)),
        ]),
        Line::from(vec![
            Span::styled("  Total: ", Style::default().fg(Color::DarkGray)),
            Span::raw(fmt_bytes(m.total_bytes)),
        ]),
        Line::from(vec![
            Span::styled("  Cache: ", Style::default().fg(Color::DarkGray)),
            Span::raw(fmt_bytes(m.cache_bytes)),
        ]),
        Line::from(vec![
            Span::styled("  Swap:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(fmt_bytes(m.swap_used_bytes),
                if m.swap_used_bytes > 0 { Style::default().fg(Color::Red) }
                else                     { Style::default().fg(Color::Green) }),
        ]),
    ];

    f.render_widget(
        Paragraph::new(info)
            .block(Block::default().borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(Span::styled(
                    format!(" Memory {pct:.0}% "),
                    Style::default().fg(Color::Magenta).bold()))),
        chunks[0]);

    let data: Vec<u64> = app.mem_history.as_slice().iter()
        .map(|v| (*v * 100.0) as u64).collect();
    f.render_widget(
        Sparkline::default().data(&data).max(10000)
            .style(Style::default().fg(Color::Magenta))
            .block(Block::default().borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(Span::styled(" Memory History ",
                    Style::default().fg(Color::Magenta).bold()))),
        chunks[1]);
}

// ------------------------------------------------------------------ //
//  Network                                                            //
// ------------------------------------------------------------------ //
fn render_network(f: &mut Frame, area: Rect, app: &App) {
    let snap = match &app.latest_snapshot {
        Some(s) if !s.net.is_empty() => s,
        _ => {
            let p = Paragraph::new("  No network interfaces detected")
                .block(Block::default().borders(Borders::ALL)
                    .border_style(section_border(app, MonitorSection::Net))
                    .title(net_title(app, 0, 0)));
            f.render_widget(p, area);
            return;
        }
    };

    let total   = snap.net.len();
    // Each iface gets a column; fit as many as the width allows (min 20 chars each)
    let inner_w  = area.width.saturating_sub(2) as usize;
    let col_min  = 24usize;
    let visible  = (inner_w / col_min).max(1).min(total);

    let max_scroll = total.saturating_sub(visible);
    let scroll     = app.monitor_net_scroll.min(max_scroll);
    let start      = scroll;
    let end        = (start + visible).min(total);

    let constraints: Vec<Constraint> = (start..end)
        .map(|_| Constraint::Ratio(1, visible as u32))
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(section_border(app, MonitorSection::Net))
        .title(net_title(app, total, visible))
        .title_bottom(Span::styled(
            scroll_hint(scroll, total, visible),
            Style::default().fg(Color::Yellow)));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if constraints.is_empty() { return; }

    let iface_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(inner);

    for (i, net_idx) in (start..end).enumerate() {
        if i >= iface_chunks.len() { break; }
        let net     = &snap.net[net_idx];
        let passthru = if net.passthru_active { " [PASSTHRU]" } else { "" };

        let info = vec![
            Line::from(vec![
                Span::styled("  RX: ", Style::default().fg(Color::DarkGray)),
                Span::styled(fmt_bytes(net.rx_bytes), Style::default().fg(Color::Green)),
            ]),
            Line::from(vec![
                Span::styled("  TX: ", Style::default().fg(Color::DarkGray)),
                Span::styled(fmt_bytes(net.tx_bytes), Style::default().fg(Color::Blue)),
            ]),
            Line::from(vec![
                Span::styled(format!("  pkts {}/{}",
                    net.rx_packets, net.tx_packets),
                    Style::default().fg(Color::DarkGray)),
            ]),
            Line::from(vec![
                Span::styled("  Drop: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("↓{} ↑{}", net.rx_dropped, net.tx_dropped),
                    if net.rx_dropped > 0 || net.tx_dropped > 0 {
                        Style::default().fg(Color::Red)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    }),
            ]),

        ];

        f.render_widget(
            Paragraph::new(info)
                .block(Block::default().borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::DarkGray))
                    .title(Span::styled(
                        format!(" {}{} ", net.iface, passthru),
                        Style::default().fg(Color::Yellow).bold()))),
            iface_chunks[i]);
    }
}

fn net_title<'a>(app: &App, total: usize, visible: usize) -> Span<'a> {
    let focused = app.monitor_section == MonitorSection::Net;
    let style   = if focused { Style::default().fg(Color::Cyan).bold() }
                  else       { Style::default().fg(Color::White).bold() };
    let hint = if focused && total > visible { " [←→ switch  ↑↓ scroll] " } else { "" };
    Span::styled(format!(" Network ({total} ifaces) {hint}"), style)
}

// ------------------------------------------------------------------ //
//  Disk + Capture latency                                             //
// ------------------------------------------------------------------ //
fn render_disk_and_latency(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);

    render_disk(f, chunks[0], app);
    render_latency(f, chunks[1], app);
}

fn render_disk(f: &mut Frame, area: Rect, app: &App) {
    let snap = match &app.latest_snapshot {
        Some(s) => s,
        None => {
            let p = Paragraph::new("  No disk data")
                .block(Block::default().borders(Borders::ALL)
                    .border_style(section_border(app, MonitorSection::Disk))
                    .title(disk_title(app, 0, 0)));
            f.render_widget(p, area);
            return;
        }
    };

    let total   = snap.disk.len();
    let inner_h = area.height.saturating_sub(2) as usize;
    // Each disk entry = 1 line
    let visible = inner_h.max(1);

    let max_scroll = total.saturating_sub(visible);
    let scroll     = app.monitor_disk_scroll.min(max_scroll);
    let start      = scroll;
    let end        = (start + visible).min(total);

    let disk_lines: Vec<Line> = snap.disk[start..end].iter().map(|d| {
        Line::from(vec![
            Span::styled(format!("  {}: ", d.device),
                Style::default().fg(Color::White).bold()),
            Span::styled(
                format!("R:{} W:{} IOq:{} t:{}ms",
                    fmt_bytes(d.read_bytes),
                    fmt_bytes(d.write_bytes),
                    d.io_in_progress,
                    d.io_time_ms),
                Style::default().fg(Color::DarkGray)),
        ])
    }).collect();

    let disk_lines = if disk_lines.is_empty() {
        vec![Line::from("  No disk devices detected")]
    } else {
        disk_lines
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(section_border(app, MonitorSection::Disk))
        .title(disk_title(app, total, visible))
        .title_bottom(Span::styled(
            scroll_hint(scroll, total, visible),
            Style::default().fg(Color::Yellow)));

    f.render_widget(Paragraph::new(disk_lines).block(block), area);
}

fn disk_title<'a>(app: &App, total: usize, visible: usize) -> Span<'a> {
    let focused = app.monitor_section == MonitorSection::Disk;
    let style   = if focused { Style::default().fg(Color::Cyan).bold() }
                  else       { Style::default().fg(Color::White).bold() };
    let hint = if focused && total > visible { " [←→ switch  ↑↓ scroll] " } else { "" };
    Span::styled(format!(" Disk I/O ({total} devs) {hint}"), style)
}

fn render_latency(f: &mut Frame, area: Rect, app: &App) {
    let data: Vec<u64> = app.capture_latency_history.as_slice().iter()
        .map(|v| *v as u64).collect();
    let last = app.capture_latency_history.last();

    f.render_widget(
        Sparkline::default()
            .data(&data)
            .style(Style::default().fg(Color::Green))
            .block(Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(Span::styled(
                    format!(" Capture Latency {last:.1} μs "),
                    Style::default().fg(Color::Green).bold()))),
        area);
}