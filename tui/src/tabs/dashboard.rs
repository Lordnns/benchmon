//! Dashboard tab — system overview, verification status, quick stats.

use ratatui::{
    prelude::*,
    widgets::*,
};
use crate::app::App;

fn status_span(ok: bool) -> Span<'static> {
    if ok {
        Span::styled("  ✓  ", Style::default().fg(Color::Green).bold())
    } else {
        Span::styled("  ✗  ", Style::default().fg(Color::Red).bold())
    }
}

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8),  // Header / system info
            Constraint::Length(14), // Verification checklist
            Constraint::Min(6),    // Live stats
        ])
        .split(area);

    // ---- System Info ----
    let v = app.verify.as_ref();

    let sys_info = vec![
        Line::from(vec![
            Span::styled("  CPU:      ", Style::default().fg(Color::DarkGray)),
            Span::raw(v.map(|v| v.cpu_model.as_str()).unwrap_or("unknown")),
        ]),
        Line::from(vec![
            Span::styled("  Kernel:   ", Style::default().fg(Color::DarkGray)),
            Span::raw(v.map(|v| v.kernel_version.as_str()).unwrap_or("?")),
        ]),
        Line::from(vec![
            Span::styled("  Freq:     ", Style::default().fg(Color::DarkGray)),
            Span::raw(format!("{} MHz",
                v.map(|v| v.actual_freq_mhz).unwrap_or(0))),
        ]),
        Line::from(vec![
            Span::styled("  RAM:      ", Style::default().fg(Color::DarkGray)),
            Span::raw(format!("{} / {} MB",
                v.map(|v| v.available_ram_mb).unwrap_or(0),
                v.map(|v| v.total_ram_mb).unwrap_or(0))),
        ]),
        Line::from(vec![
            Span::styled("  Platform: ", Style::default().fg(Color::DarkGray)),
            if v.map(|v| v.running_bare_metal).unwrap_or(false) {
                Span::styled("Bare Metal", Style::default().fg(Color::Green))
            } else {
                Span::styled(
                    format!("VM ({})",
                        v.map(|v| v.hypervisor.as_str()).unwrap_or("?")),
                    Style::default().fg(Color::Yellow))
            },
        ]),
    ];

    let sys_block = Paragraph::new(sys_info)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(Span::styled(" System ",
                Style::default().fg(Color::Cyan).bold())));
    f.render_widget(sys_block, chunks[0]);

    // ---- Verification Checklist ----
    let checks: Vec<Line> = if let Some(v) = v {
        vec![
            Line::from(vec![
                status_span(v.smt_disabled),
                Span::raw("SMT / Hyper-Threading disabled"),
            ]),
            Line::from(vec![
                status_span(v.cores_isolated),
                Span::raw(format!("CPU cores isolated (isolcpus) {:?}",
                    v.isolated_core_list)),
            ]),
            Line::from(vec![
                status_span(v.nohz_full_active),
                Span::raw("nohz_full (tickless) active"),
            ]),
            Line::from(vec![
                status_span(v.frequency_boost_off),
                Span::raw("Frequency boost disabled"),
            ]),
            Line::from(vec![
                status_span(v.swap_off),
                Span::raw("Swap disabled"),
            ]),
            Line::from(vec![
                status_span(v.irqbalance_stopped),
                Span::raw("irqbalance stopped"),
            ]),
            Line::from(vec![
                status_span(v.running_bare_metal),
                Span::raw("Running on bare metal"),
            ]),
            Line::from(vec![
                status_span(v.ns_server_exists && v.ns_client_exists),
                Span::raw("Network namespaces (ns-server, ns-client)"),
            ]),
            Line::from(vec![
                status_span(v.netem_active),
                Span::raw("NetEm active on veth"),
            ]),
            Line::from(vec![
                status_span(v.offloading_disabled),
                Span::raw("TSO/GSO/GRO offloading disabled"),
            ]),
        ]
    } else {
        vec![Line::from("  Verification not run yet...")]
    };

    let ready = v.map(|v| v.all_checks_passed).unwrap_or(false);
    let title_style = if ready {
        Style::default().fg(Color::Green).bold()
    } else {
        Style::default().fg(Color::Yellow).bold()
    };
    let title = if ready {
        " Checklist — ALL PASSED ✓ "
    } else {
        " Checklist — ISSUES FOUND "
    };

    let check_block = Paragraph::new(checks)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(Span::styled(title, title_style)));
    f.render_widget(check_block, chunks[1]);

    // ---- Live Stats ----
    let snap_info = if let Some(ref snap) = app.latest_snapshot {
        vec![
            Line::from(vec![
                Span::styled("  Capture latency: ",
                    Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{:.1} μs", snap.capture_latency_ns as f64 / 1000.0),
                    Style::default().fg(Color::Green)),
            ]),
            Line::from(vec![
                Span::styled("  CPU cores:    ",
                    Style::default().fg(Color::DarkGray)),
                Span::raw(format!("{}", snap.cpu.len())),
            ]),
            Line::from(vec![
                Span::styled("  Net ifaces:   ",
                    Style::default().fg(Color::DarkGray)),
                Span::raw(snap.net.iter()
                    .map(|n| n.iface.as_str())
                    .collect::<Vec<_>>().join(", ")),
            ]),
            Line::from(vec![
                Span::styled("  Disk devices: ",
                    Style::default().fg(Color::DarkGray)),
                Span::raw(snap.disk.iter()
                    .map(|d| d.device.as_str())
                    .collect::<Vec<_>>().join(", ")),
            ]),
            Line::from(vec![
                Span::styled("  Tick #:       ",
                    Style::default().fg(Color::DarkGray)),
                Span::raw(format!("{}", app.tick_count)),
            ]),
        ]
    } else {
        vec![Line::from("  Monitor not active — run as root for full data")]
    };

    let stats_block = Paragraph::new(snap_info)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(Span::styled(" Live Monitor ",
                Style::default().fg(Color::Cyan).bold())));
    f.render_widget(stats_block, chunks[2]);
}
