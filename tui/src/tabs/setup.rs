//! Setup tab — interactive configuration with inline editing,
//! config snapshot picker for teardown, and reboot-required modal.

use ratatui::{
    prelude::*,
    widgets::*,
};
use crate::app::{App, LogLevel};
use crate::snapshot_store;

const SETUP_ITEMS: &[(&str, FieldKind)] = &[
    ("Isolated cores (e.g. 2,3,4,5)",   FieldKind::Edit),
    ("Housekeeping core",                FieldKind::Edit),
    ("Disable SMT",                      FieldKind::Toggle),
    ("Disable frequency boost",          FieldKind::Toggle),
    ("Max C-state",                      FieldKind::Edit),
    ("Stop irqbalance",                  FieldKind::Toggle),
    ("Disable swap",                     FieldKind::Toggle),
    ("Multi-user target",                FieldKind::Toggle),
    ("NS server name",                   FieldKind::Edit),
    ("NS client name",                   FieldKind::Edit),
    ("Server IP (CIDR)",                 FieldKind::Edit),
    ("Client IP (CIDR)",                 FieldKind::Edit),
    ("NetEm delay (ms)",                 FieldKind::Edit),
    ("NetEm jitter (ms)",                FieldKind::Edit),
    ("NetEm loss (%)",                   FieldKind::Edit),
    ("Disable offloading",               FieldKind::Toggle),
    ("Server cores (e.g. 2,4)",          FieldKind::Edit),
    ("Client cores (e.g. 3,5)",          FieldKind::Edit),
    ("RT priority (chrt -f)",            FieldKind::Edit),
    ("Disable ASLR",                     FieldKind::Toggle),
    ("Tune net buffers",                 FieldKind::Toggle),
    ("Drop caches on apply",             FieldKind::Toggle),
    ("Stop timesyncd",                   FieldKind::Toggle),
    ("─── Actions ───",                  FieldKind::Separator),
    ("▶  APPLY SETUP",                   FieldKind::Action),
    ("▶  TEARDOWN",                      FieldKind::Action),
    ("▶  REFRESH VERIFY",                FieldKind::Action),
];

#[derive(Clone, Copy, PartialEq)]
enum FieldKind { Edit, Toggle, Separator, Action }

// ------------------------------------------------------------------ //
//  Main render                                                        //
// ------------------------------------------------------------------ //

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    // Render the base form first
    render_form(f, area, app);

    // Overlays — render on top
    if app.teardown_picker_active {
        render_teardown_picker(f, area, app);
    }
    if app.reboot_modal {
        render_reboot_modal(f, area, app);
    }
}

fn render_form(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);

    // ---- Left: config form ----
    let items: Vec<ListItem> = SETUP_ITEMS.iter().enumerate().map(|(i, (label, kind))| {
        let is_selected = i == app.setup_selected_item;
        let is_editing  = is_selected && app.setup_editing;

        let row_style = if is_editing {
            Style::default().fg(Color::Black).bg(Color::Cyan).bold()
        } else if is_selected {
            Style::default().fg(Color::Yellow).bold().bg(Color::DarkGray)
        } else {
            match kind {
                FieldKind::Action    => Style::default().fg(Color::Cyan).bold(),
                FieldKind::Separator => Style::default().fg(Color::DarkGray),
                _                    => Style::default(),
            }
        };

        let content = if is_editing {
            let cursor = if app.tick_count % 10 < 5 { "█" } else { " " };
            Line::from(vec![
                Span::styled(format!("  {label}: "), row_style),
                Span::styled(
                    format!("{}{}", app.setup_edit_buffer, cursor),
                    Style::default().fg(Color::Black).bg(Color::Cyan).bold(),
                ),
            ])
        } else {
            let value = get_config_value(app, i);
            if value.is_empty() {
                Line::from(Span::styled(format!("  {label}"), row_style))
            } else {
                let val_style = if is_selected {
                    row_style.patch(Style::default().fg(Color::Green))
                } else {
                    Style::default().fg(Color::Green)
                };
                Line::from(vec![
                    Span::styled(format!("  {label}: "), row_style),
                    Span::styled(value, val_style),
                ])
            }
        };

        ListItem::new(content)
    }).collect();

    let hint = if app.setup_editing {
        " Enter=confirm  Esc=cancel "
    } else {
        " ↑↓=navigate  Enter=edit/toggle/run "
    };

    let list = List::new(items)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(
                if app.setup_editing { Color::Cyan } else { Color::DarkGray }
            ))
            .title(Span::styled(" Setup Configuration ",
                Style::default().fg(Color::Cyan).bold()))
            .title_bottom(Span::styled(hint,
                Style::default().fg(Color::DarkGray))));

    f.render_widget(list, chunks[0]);

    // ---- Right: result panel ----
    let result_lines = if let Some(ref r) = app.setup_result {
        let status_color = match r.status {
            crate::ffi::Status::Ok        => Color::Green,
            crate::ffi::Status::ErrReboot => Color::Yellow,
            _                             => Color::Red,
        };

        let mut lines = vec![
            Line::from(Span::styled(
                format!("  Status: {:?}", r.status),
                Style::default().fg(status_color).bold())),
            Line::default(),
        ];

        if r.reboot_required {
            lines.push(Line::from(Span::styled(
                "  ⚠ REBOOT REQUIRED — press R in this modal",
                Style::default().fg(Color::Yellow).bold())));
            lines.push(Line::default());
        }

        for (label, ok) in [
            ("GRUB modified",           r.grub_modified),
            ("SMT disabled",            r.smt_disabled),
            ("IRQ migrated",            r.irq_migrated),
            ("Namespaces created",      r.namespaces_created),
            ("NetEm applied",           r.netem_applied),
            ("Offloading disabled",     r.offloading_disabled),
            ("Swap disabled",           r.swap_disabled),
            ("Services stopped",        r.services_stopped),
            ("Frequency locked",        r.frequency_locked),
            ("Sysctl tuned",            r.sysctl_tuned),
            ("Caches dropped",          r.caches_dropped),
            ("Process isolation ready", r.process_isolation_ready),
        ] {
            let (icon, color) = if ok { ("✓", Color::Green) } else { ("·", Color::DarkGray) };
            lines.push(Line::from(Span::styled(
                format!("  {icon} {label}"), Style::default().fg(color))));
        }

        if !r.message.is_empty() {
            lines.push(Line::default());
            lines.push(Line::from(Span::styled(
                " ── Log ──", Style::default().fg(Color::DarkGray))));
            for line in r.message.lines() {
                lines.push(Line::from(Span::styled(
                    format!("  {line}"),
                    Style::default().fg(Color::DarkGray))));
            }
        }
        lines
    } else {
        // Show metric log info if active
        let log_line = if app.logging_active {
            if let Some(ref lg) = app.logger {
                format!("  ● Logging → {}",
                    lg.path.split('/').last().unwrap_or(&lg.path))
            } else { String::new() }
        } else {
            "  ○ Logging off (log start in terminal)".into()
        };

        vec![
            Line::from("  No setup has been run yet."),
            Line::default(),
            Line::from(Span::styled(
                "  ↑/↓ to navigate, Enter on a field to edit it.",
                Style::default().fg(Color::DarkGray))),
            Line::default(),
            Line::from(Span::styled(
                "  Config snapshot saved before every Apply.",
                Style::default().fg(Color::DarkGray))),
            Line::from(Span::styled(
                "  Teardown will let you pick which to restore.",
                Style::default().fg(Color::DarkGray))),
            Line::default(),
            Line::from(Span::styled(
                "  Some actions (GRUB) require a reboot.",
                Style::default().fg(Color::Yellow))),
            Line::default(),
            Line::from(Span::styled(log_line,
                Style::default().fg(Color::Cyan))),
        ]
    };

    let result_block = Paragraph::new(result_lines)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(Span::styled(" Setup Result ",
                Style::default().fg(Color::Cyan).bold())))
        .wrap(Wrap { trim: false });
    f.render_widget(result_block, chunks[1]);
}

// ------------------------------------------------------------------ //
//  Reboot modal overlay                                               //
// ------------------------------------------------------------------ //

fn render_reboot_modal(f: &mut Frame, area: Rect, _app: &App) {
    let w = 50u16;
    let h = 9u16;
    let x = area.x + area.width.saturating_sub(w) / 2;
    let y = area.y + area.height.saturating_sub(h) / 2;
    let modal_area = Rect { x, y, width: w.min(area.width), height: h.min(area.height) };

    // Dim background by rendering a clear block
    f.render_widget(Clear, modal_area);

    let text = vec![
        Line::default(),
        Line::from(Span::styled(
            "  GRUB was modified.",
            Style::default().fg(Color::White))),
        Line::from(Span::styled(
            "  Kernel params apply only after reboot.",
            Style::default().fg(Color::White))),
        Line::default(),
        Line::from(vec![
            Span::styled("     [ R ] Reboot now  ",
                Style::default().fg(Color::Black).bg(Color::Red).bold()),
            Span::raw("   "),
            Span::styled("  [ Esc ] Later  ",
                Style::default().fg(Color::Black).bg(Color::DarkGray).bold()),
        ]),
        Line::default(),
    ];

    let block = Paragraph::new(text)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow).bold())
            .title(Span::styled(
                " ⚠  REBOOT REQUIRED ",
                Style::default().fg(Color::Yellow).bold())))
        .alignment(Alignment::Left);

    f.render_widget(block, modal_area);
}

// ------------------------------------------------------------------ //
//  Teardown config picker overlay                                     //
// ------------------------------------------------------------------ //

fn render_teardown_picker(f: &mut Frame, area: Rect, app: &App) {
    let w = (area.width * 80 / 100).max(60);
    let h = (area.height * 70 / 100).max(12);
    let x = area.x + area.width.saturating_sub(w) / 2;
    let y = area.y + area.height.saturating_sub(h) / 2;
    let picker_area = Rect { x, y, width: w.min(area.width), height: h.min(area.height) };

    f.render_widget(Clear, picker_area);

    if app.teardown_picker_preview {
        // Split: list on left, preview on right
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(picker_area);
        render_picker_list(f, chunks[0], app);
        render_picker_preview(f, chunks[1], app);
    } else {
        render_picker_list(f, picker_area, app);
    }
}

fn render_picker_list(f: &mut Frame, area: Rect, app: &App) {
    let items: Vec<ListItem> = app.teardown_snapshots.iter().enumerate().map(|(i, snap)| {
        let is_sel = i == app.teardown_picker_idx;
        let style = if is_sel {
            Style::default().fg(Color::Yellow).bg(Color::DarkGray).bold()
        } else {
            Style::default()
        };
        let prefix = if is_sel { " ▶ " } else { "   " };
        ListItem::new(Line::from(vec![
            Span::styled(format!("{}{}", prefix, snap.timestamp_str),
                style.fg(Color::Cyan)),
            Span::styled(format!("  {}", snap.preview), style.fg(Color::White)),
        ]))
    }).collect();

    let hint = " ↑↓=select  Enter=restore  i=preview  Esc=cancel ";

    let list = List::new(items)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan).bold())
            .title(Span::styled(
                format!(" Restore Config  ({} snapshots) ",
                    app.teardown_snapshots.len()),
                Style::default().fg(Color::Cyan).bold()))
            .title_bottom(Span::styled(hint,
                Style::default().fg(Color::DarkGray))));

    f.render_widget(list, area);
}

fn render_picker_preview(f: &mut Frame, area: Rect, app: &App) {
    let snap = app.teardown_snapshots.get(app.teardown_picker_idx);

    let lines: Vec<Line> = match snap.and_then(|s| s.config.as_ref()) {
        None => vec![Line::from("  (no config data)")],
        Some(c) => vec![
            Line::from(Span::styled("  Isolated cores:", Style::default().fg(Color::DarkGray))),
            Line::from(format!("    {:?}", c.isolated_cores)),
            Line::from(Span::styled("  Housekeeping core:", Style::default().fg(Color::DarkGray))),
            Line::from(format!("    {}", c.housekeeping_core)),
            Line::from(Span::styled("  SMT disabled:", Style::default().fg(Color::DarkGray))),
            Line::from(format!("    {}", c.disable_smt)),
            Line::from(Span::styled("  Freq boost off:", Style::default().fg(Color::DarkGray))),
            Line::from(format!("    {}", c.disable_frequency_boost)),
            Line::from(Span::styled("  Swap off:", Style::default().fg(Color::DarkGray))),
            Line::from(format!("    {}", c.disable_swap)),
            Line::from(Span::styled("  NetEm delay/jitter:", Style::default().fg(Color::DarkGray))),
            Line::from(format!("    {}ms / {}ms", c.netem_delay_ms, c.netem_jitter_ms)),
            Line::from(Span::styled("  NetEm loss:", Style::default().fg(Color::DarkGray))),
            Line::from(format!("    {:.2}%", c.netem_loss_pct)),
            Line::from(Span::styled("  Namespaces:", Style::default().fg(Color::DarkGray))),
            Line::from(format!("    {} / {}", c.ns_server, c.ns_client)),
            Line::from(Span::styled("  IPs:", Style::default().fg(Color::DarkGray))),
            Line::from(format!("    {} ↔ {}", c.server_ip, c.client_ip)),
        ],
    };

    let block = Paragraph::new(lines)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(Span::styled(" Preview ",
                Style::default().fg(Color::Cyan).bold())))
        .wrap(Wrap { trim: false });

    f.render_widget(block, area);
}

// ------------------------------------------------------------------ //
//  Config value display helper                                        //
// ------------------------------------------------------------------ //

fn get_config_value(app: &App, idx: usize) -> String {
    let c = &app.setup_config;
    match idx {
        0  => c.isolated_cores.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(","),
        1  => format!("{}", c.housekeeping_core),
        2  => bool_str(c.disable_smt),
        3  => bool_str(c.disable_frequency_boost),
        4  => format!("{}", c.max_cstate),
        5  => bool_str(c.stop_irqbalance),
        6  => bool_str(c.disable_swap),
        7  => bool_str(c.isolate_multiuser),
        8  => c.ns_server.clone(),
        9  => c.ns_client.clone(),
        10 => c.server_ip.clone(),
        11 => c.client_ip.clone(),
        12 => format!("{}", c.netem_delay_ms),
        13 => format!("{}", c.netem_jitter_ms),
        14 => format!("{:.2}", c.netem_loss_pct),
        15 => bool_str(c.disable_offloading),
        16 => c.server_cores.clone(),
        17 => c.client_cores.clone(),
        18 => format!("{}", c.rt_priority),
        19 => bool_str(c.disable_aslr),
        20 => bool_str(c.tune_net_buffers),
        21 => bool_str(c.drop_caches),
        22 => bool_str(c.stop_timesyncd),
        _  => String::new(),
    }
}

fn bool_str(b: bool) -> String {
    if b { "true  [ON]".into() } else { "false [OFF]".into() }
}

// ------------------------------------------------------------------ //
//  Key handlers (called from main.rs)                                 //
// ------------------------------------------------------------------ //

pub fn handle_enter(app: &mut App) {
    let (_, kind) = SETUP_ITEMS[app.setup_selected_item];
    match kind {
        FieldKind::Edit => {
            app.setup_edit_buffer = get_config_value(app, app.setup_selected_item);
            app.setup_editing = true;
        }
        FieldKind::Toggle => toggle_field(app, app.setup_selected_item),
        FieldKind::Action => run_action(app, app.setup_selected_item),
        FieldKind::Separator => {}
    }
}

pub fn commit_edit(app: &mut App) {
    let buf = app.setup_edit_buffer.trim().to_string();
    let idx = app.setup_selected_item;

    let ok = match idx {
        0 => {
            let cores: Vec<i32> = buf.split(',')
                .filter_map(|s| s.trim().parse().ok())
                .collect();
            if !cores.is_empty() {
                app.setup_config.isolated_cores = cores;
                true
            } else {
                app.log(LogLevel::Error,
                    "Isolated cores: enter comma-separated integers e.g. 2,3,4,5");
                false
            }
        }
        1 => {
            if let Ok(v) = buf.parse::<i32>() {
                app.setup_config.housekeeping_core = v; true
            } else {
                app.log(LogLevel::Error, "Housekeeping core: must be an integer");
                false
            }
        }
        4 => {
            if let Ok(v) = buf.parse::<i32>() {
                app.setup_config.max_cstate = v; true
            } else {
                app.log(LogLevel::Error, "Max C-state: must be an integer (0 = lowest)");
                false
            }
        }
        8  => { app.setup_config.ns_server = buf; true }
        9  => { app.setup_config.ns_client = buf; true }
        10 => { app.setup_config.server_ip = buf; true }
        11 => { app.setup_config.client_ip = buf; true }
        12 => {
            if let Ok(v) = buf.parse::<i32>() {
                app.setup_config.netem_delay_ms = v; true
            } else {
                app.log(LogLevel::Error, "NetEm delay: must be an integer (ms)");
                false
            }
        }
        13 => {
            if let Ok(v) = buf.parse::<i32>() {
                app.setup_config.netem_jitter_ms = v; true
            } else {
                app.log(LogLevel::Error, "NetEm jitter: must be an integer (ms)");
                false
            }
        }
        14 => {
            if let Ok(v) = buf.parse::<f64>() {
                app.setup_config.netem_loss_pct = v; true
            } else {
                app.log(LogLevel::Error, "NetEm loss: must be a decimal e.g. 0.10");
                false
            }
        }
        16 => { app.setup_config.server_cores = buf; true }
        17 => { app.setup_config.client_cores = buf; true }
        18 => {
            if let Ok(v) = buf.parse::<i32>() {
                app.setup_config.rt_priority = v; true
            } else {
                app.log(LogLevel::Error, "RT priority: must be an integer (1-99)");
                false
            }
        }
        _ => false,
    };

    if ok {
        app.log(LogLevel::Success, &format!("Updated: {}", SETUP_ITEMS[idx].0));
    }

    app.setup_editing = false;
    app.setup_edit_buffer.clear();
}

fn toggle_field(app: &mut App, idx: usize) {
    match idx {
        2  => app.setup_config.disable_smt             = !app.setup_config.disable_smt,
        3  => app.setup_config.disable_frequency_boost = !app.setup_config.disable_frequency_boost,
        5  => app.setup_config.stop_irqbalance          = !app.setup_config.stop_irqbalance,
        6  => app.setup_config.disable_swap             = !app.setup_config.disable_swap,
        7  => app.setup_config.isolate_multiuser        = !app.setup_config.isolate_multiuser,
        15 => app.setup_config.disable_offloading       = !app.setup_config.disable_offloading,
        19 => app.setup_config.disable_aslr             = !app.setup_config.disable_aslr,
        20 => app.setup_config.tune_net_buffers         = !app.setup_config.tune_net_buffers,
        21 => app.setup_config.drop_caches              = !app.setup_config.drop_caches,
        22 => app.setup_config.stop_timesyncd           = !app.setup_config.stop_timesyncd,
        _  => {}
    }
}

fn run_action(app: &mut App, idx: usize) {
    match idx {
        // APPLY SETUP — index 24 (was 17)
        24 => {
            // Save a config snapshot BEFORE applying
             match snapshot_store::save(&snapshot_store::capture_current_state(), "preconfig") {
                Some(path) => app.log(LogLevel::Info,
                    &format!("Pre-apply state saved: {}", path)),
                None => app.log(LogLevel::Warn,
                    "Could not save pre-apply snapshot to /tmp"),
            }

            // Save the config we are about to apply
            match snapshot_store::save(&app.setup_config, "config") {
                Some(path) => app.log(LogLevel::Info,
                    &format!("Apply config saved: {}", path)),
                None => app.log(LogLevel::Warn,
                    "Could not save apply config snapshot to /tmp"),
            }


            app.log(LogLevel::Info, "Running setup...");
            app.setup_running = true;

            if let Some(ref mut lg) = app.logger {
                lg.log_event("setup", "apply started");
            }

            let result = crate::ffi::run_setup(&app.setup_config);

            if let Some(ref mut lg) = app.logger {
                lg.log_event("setup",
                    &format!("apply finished status={:?} reboot={}",
                        result.status, result.reboot_required));
            }

            if result.reboot_required {
                app.log(LogLevel::Warn, "Setup complete — GRUB modified, REBOOT REQUIRED");
                app.setup_result = Some(result);
                app.setup_running = false;
                app.show_reboot_modal(false);
                return;
            }

            match result.status {
                crate::ffi::Status::Ok =>
                    app.log(LogLevel::Success, "Setup completed successfully"),
                crate::ffi::Status::ErrPerm =>
                    app.log(LogLevel::Error, "Setup failed: need root"),
                _ =>
                    app.log(LogLevel::Error,
                        &format!("Setup finished: {:?}", result.status)),
            }
            app.setup_result = Some(result);
            app.setup_running = false;
            snapshot_store::save_active(&app.setup_config);
            app.refresh_verify();
        }
        // TEARDOWN — index 25 (was 18)
        25 => {
            app.open_teardown_picker();
        }
        // REFRESH VERIFY — index 26 (was 19)
        26 => {
            app.refresh_verify();
        }
        _ => {}
    }
}

pub fn item_count() -> usize {
    SETUP_ITEMS.len()
}