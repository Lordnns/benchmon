//! benchmon TUI — main entry point
//!
//! Usage:
//!   benchmon                          Launch interactive TUI
//!   benchmon service start            Start background daemon (transient)
//!   benchmon service stop             Stop background daemon
//!   benchmon service enable           Install + enable systemd unit (persistent)
//!   benchmon service disable          Remove systemd unit + stop daemon
//!   benchmon service status           Show service status
//!   benchmon --daemon [transient|persistent]   (internal — used by service commands)

mod ffi;
mod app;
mod tabs;
mod logger;
mod snapshot_store;
mod service;

use std::io;
use std::time::{Duration, Instant};
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture,
        Event, KeyCode, KeyEvent, KeyModifiers,
        MouseEvent, MouseEventKind,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode,
               EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{ prelude::*, widgets::* };
use app::{App, MonitorSection, Tab};

const TICK_RATE: Duration = Duration::from_millis(100);

fn main() -> color_eyre::Result<()> {
    let args: Vec<String> = std::env::args().collect();

    // ---------------------------------------------------------------- //
    //  Non-TUI entry points                                             //
    // ---------------------------------------------------------------- //

    // Internal: daemon mode — no TUI, blocks until killed.
    if args.get(1).map(|s| s.as_str()) == Some("--daemon") {
        let mode = match args.get(2).map(|s| s.as_str()) {
            Some("persistent") => service::ServiceMode::Persistent,
            _                  => service::ServiceMode::Transient,
        };
        service::run_daemon(mode);
        return Ok(());
    }

    // Public: service management subcommands.
    if args.get(1).map(|s| s.as_str()) == Some("service") {
        let result: Result<String, String> = match args.get(2).map(|s| s.as_str()) {
            Some("start")   => service::start().map(|_| "Daemon started.".into()),
            Some("stop")    => service::stop().map(|_| "Daemon stopped.".into()),
            Some("enable")  => service::enable().map(|_| "Service enabled and started.".into()),
            Some("disable") => service::disable().map(|_| "Service disabled and unit removed.".into()),
            Some("status")  => { println!("{}", service::status_string()); return Ok(()); }
            _ => {
                eprintln!(
                    "Usage: benchmon service [start|stop|enable|disable|status]\n\
                     \n\
                     start    — start background daemon (transient, no reboot persistence)\n\
                     stop     — stop the daemon\n\
                     enable   — install systemd unit + start (persistent, restarts on boot)\n\
                     disable  — stop + remove systemd unit\n\
                     status   — show daemon PID, mode, log path"
                );
                return Ok(());
            }
        };
        match result {
            Ok(msg) => println!("{msg}"),
            Err(e)  => { eprintln!("Error: {e}"); std::process::exit(1); }
        }
        return Ok(());
    }

    // ---------------------------------------------------------------- //
    //  TUI mode (default)                                               //
    // ---------------------------------------------------------------- //

    color_eyre::install()?;
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
    let mut last_tick = Instant::now();

    loop {
        terminal.draw(|f| ui(f, &app))?;

        let timeout = TICK_RATE.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)? {
            match event::read()? {
                Event::Key(key)     => handle_key(&mut app, key),
                Event::Mouse(mouse) => handle_mouse(&mut app, mouse),
                _ => {}
            }
        }

        if last_tick.elapsed() >= TICK_RATE {
            app.tick();
            last_tick = Instant::now();
        }

        if app.should_quit { break; }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;
    Ok(())
}

// ------------------------------------------------------------------ //
//  Scroll helpers                                                     //
// ------------------------------------------------------------------ //

fn scroll_up(offset: &mut usize, n: usize, total: usize) {
    *offset = offset.saturating_add(n).min(total);
}
fn scroll_down(offset: &mut usize, n: usize) {
    *offset = offset.saturating_sub(n);
}

fn monitor_scroll(app: &mut App, up: bool, n: usize) {
    let total_cpu  = app.latest_snapshot.as_ref().map(|s| s.cpu.len()).unwrap_or(0);
    let total_net  = app.latest_snapshot.as_ref().map(|s| s.net.len()).unwrap_or(0);
    let total_disk = app.latest_snapshot.as_ref().map(|s| s.disk.len()).unwrap_or(0);
    match app.monitor_section {
        MonitorSection::Cpu => {
            if up { scroll_up(&mut app.monitor_cpu_scroll, n, total_cpu); }
            else  { scroll_down(&mut app.monitor_cpu_scroll, n); }
        }
        MonitorSection::Net => {
            if up { scroll_up(&mut app.monitor_net_scroll, n, total_net); }
            else  { scroll_down(&mut app.monitor_net_scroll, n); }
        }
        MonitorSection::Disk => {
            if up { scroll_up(&mut app.monitor_disk_scroll, n, total_disk); }
            else  { scroll_down(&mut app.monitor_disk_scroll, n); }
        }
    }
}

// ------------------------------------------------------------------ //
//  Mouse                                                              //
// ------------------------------------------------------------------ //

fn handle_mouse(app: &mut App, mouse: MouseEvent) {
    match mouse.kind {
        MouseEventKind::ScrollUp => match app.active_tab {
            Tab::Terminal => scroll_up(&mut app.terminal_scroll, 3, app.terminal_output.len()),
            Tab::Logs     => scroll_up(&mut app.log_scroll, 3, app.logs.len()),
            Tab::Monitor  => monitor_scroll(app, true, 3),
            _ => {}
        },
        MouseEventKind::ScrollDown => match app.active_tab {
            Tab::Terminal => scroll_down(&mut app.terminal_scroll, 3),
            Tab::Logs     => scroll_down(&mut app.log_scroll, 3),
            Tab::Monitor  => monitor_scroll(app, false, 3),
            _ => {}
        },
        _ => {}
    }
}

// ------------------------------------------------------------------ //
//  Keys                                                               //
// ------------------------------------------------------------------ //

fn handle_key(app: &mut App, key: KeyEvent) {

    // ── Root warning modal — first priority ──────────────────────────
    if app.root_warning_modal {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => app.dismiss_root_warning(),
            KeyCode::Char('q') | KeyCode::Char('Q') => app.should_quit = true,
            _ => {}
        }
        return;
    }

    // ── Reboot modal ─────────────────────────────────────────────────
    if app.reboot_modal {
        match key.code {
            KeyCode::Char('r') | KeyCode::Char('R') => app.reboot_now(),
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') =>
                app.dismiss_reboot_modal(),
            _ => {}
        }
        return;
    }

    // ── Teardown picker ──────────────────────────────────────────────
    if app.teardown_picker_active {
        match key.code {
            KeyCode::Up   | KeyCode::Char('k') => app.teardown_picker_up(),
            KeyCode::Down | KeyCode::Char('j') => app.teardown_picker_down(),
            KeyCode::Enter                      => app.teardown_picker_confirm(),
            KeyCode::Char('i') | KeyCode::Char('I') =>
                app.teardown_picker_preview = !app.teardown_picker_preview,
            KeyCode::Esc => app.teardown_picker_cancel(),
            _ => {}
        }
        return;
    }

    // ── Setup editing ────────────────────────────────────────────────
    if app.active_tab == Tab::Setup && app.setup_editing {
        match key.code {
            KeyCode::Esc       => { app.setup_editing = false; app.setup_edit_buffer.clear(); }
            KeyCode::Enter     => { tabs::setup::commit_edit(app); }
            KeyCode::Backspace => { app.setup_edit_buffer.pop(); }
            KeyCode::Char(c)   => { app.setup_edit_buffer.push(c); }
            _ => {}
        }
        return;
    }

    // ── Terminal tab ─────────────────────────────────────────────────
    if app.active_tab == Tab::Terminal {
        let total = app.terminal_output.len();
        match key.code {
            KeyCode::PageUp   => scroll_up(&mut app.terminal_scroll, 10, total),
            KeyCode::PageDown => scroll_down(&mut app.terminal_scroll, 10),
            KeyCode::Home if !app.terminal_focused => { app.terminal_scroll = total; }
            KeyCode::End  if !app.terminal_focused => { app.terminal_scroll = 0; }
            KeyCode::Up   if !app.terminal_focused => scroll_up(&mut app.terminal_scroll, 1, total),
            KeyCode::Down if !app.terminal_focused => scroll_down(&mut app.terminal_scroll, 1),
            KeyCode::Esc  if  app.terminal_focused => { app.terminal_focused = false; }
            KeyCode::Enter if  app.terminal_focused => {
                app.terminal_scroll = 0;
                app.exec_terminal_command();
            }
            KeyCode::Backspace if app.terminal_focused => {
                app.cmd_history_idx = None;
                app.terminal_input.pop();
            }
            KeyCode::Char(c) if app.terminal_focused => {
                app.cmd_history_idx = None;
                app.terminal_input.push(c);
            }
            KeyCode::Up   if app.terminal_focused => { app.history_prev(); }
            KeyCode::Down if app.terminal_focused => { app.history_next(); }
            KeyCode::Enter if !app.terminal_focused => { app.terminal_focused = true; }
            KeyCode::Char('q') | KeyCode::Char('Q') if !app.terminal_focused =>
                { app.should_quit = true; }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) =>
                { app.should_quit = true; }
            KeyCode::Tab     => { app.active_tab = app.active_tab.next(); }
            KeyCode::BackTab => { app.active_tab = app.active_tab.prev(); }
            _ => {}
        }
        return;
    }

    // ── Logs tab ─────────────────────────────────────────────────────
    if app.active_tab == Tab::Logs {
        let total = app.logs.len();
        match key.code {
            KeyCode::PageUp   => scroll_up(&mut app.log_scroll, 10, total),
            KeyCode::PageDown => scroll_down(&mut app.log_scroll, 10),
            KeyCode::Home     => { app.log_scroll = total; }
            KeyCode::End      => { app.log_scroll = 0; }
            KeyCode::Up       => scroll_up(&mut app.log_scroll, 1, total),
            KeyCode::Down     => scroll_down(&mut app.log_scroll, 1),
            KeyCode::Char('q') | KeyCode::Char('Q') => { app.should_quit = true; }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) =>
                { app.should_quit = true; }
            KeyCode::Tab     => { app.active_tab = app.active_tab.next(); }
            KeyCode::BackTab => { app.active_tab = app.active_tab.prev(); }
            _ => {}
        }
        return;
    }

    // ── Monitor tab ──────────────────────────────────────────────────
    if app.active_tab == Tab::Monitor {
        match key.code {
            KeyCode::Left  => { app.monitor_section = app.monitor_section.prev(); }
            KeyCode::Right => { app.monitor_section = app.monitor_section.next(); }
            KeyCode::PageUp    => monitor_scroll(app, true,  10),
            KeyCode::PageDown  => monitor_scroll(app, false, 10),
            KeyCode::Up        => monitor_scroll(app, true,   1),
            KeyCode::Down      => monitor_scroll(app, false,  1),
            KeyCode::Home => {
                app.monitor_cpu_scroll  = 999;
                app.monitor_net_scroll  = 999;
                app.monitor_disk_scroll = 999;
            }
            KeyCode::End => {
                app.monitor_cpu_scroll  = 0;
                app.monitor_net_scroll  = 0;
                app.monitor_disk_scroll = 0;
            }
            KeyCode::Char('q') | KeyCode::Char('Q') => { app.should_quit = true; }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) =>
                { app.should_quit = true; }
            KeyCode::Tab     => { app.active_tab = app.active_tab.next(); }
            KeyCode::BackTab => { app.active_tab = app.active_tab.prev(); }
            _ => {}
        }
        return;
    }

    // ── Global keys ──────────────────────────────────────────────────
    match key.code {
        KeyCode::Char('q') | KeyCode::Char('Q') => { app.should_quit = true; }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) =>
            { app.should_quit = true; }
        KeyCode::F(1) => app.active_tab = Tab::Dashboard,
        KeyCode::F(2) => app.active_tab = Tab::Setup,
        KeyCode::F(3) => app.active_tab = Tab::Monitor,
        KeyCode::F(4) => app.active_tab = Tab::Logs,
        KeyCode::F(5) => app.active_tab = Tab::Terminal,
        KeyCode::Tab     => app.active_tab = app.active_tab.next(),
        KeyCode::BackTab => app.active_tab = app.active_tab.prev(),
        KeyCode::Up => {
            if app.active_tab == Tab::Setup && app.setup_selected_item > 0 {
                app.setup_selected_item -= 1;
                if app.setup_selected_item == 16 && app.setup_selected_item > 0 {
                    app.setup_selected_item -= 1;
                }
            }
        }
        KeyCode::Down => {
            if app.active_tab == Tab::Setup {
                let max = tabs::setup::item_count() - 1;
                if app.setup_selected_item < max {
                    app.setup_selected_item += 1;
                    if app.setup_selected_item == 16 { app.setup_selected_item += 1; }
                }
            }
        }
        KeyCode::Enter => {
            if app.active_tab == Tab::Setup { tabs::setup::handle_enter(app); }
        }
        KeyCode::Char('r') | KeyCode::Char('R') => { app.refresh_verify(); }
        _ => {}
    }
}

// ------------------------------------------------------------------ //
//  UI                                                                 //
// ------------------------------------------------------------------ //

fn ui(f: &mut Frame, app: &App) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(1),
        ])
        .split(f.area());

    render_tab_bar(f, layout[0], app);

    match app.active_tab {
        Tab::Dashboard => tabs::dashboard::render(f, layout[1], app),
        Tab::Setup     => tabs::setup::render(f, layout[1], app),
        Tab::Monitor   => tabs::monitor::render(f, layout[1], app),
        Tab::Logs      => tabs::logs::render(f, layout[1], app),
        Tab::Terminal  => tabs::terminal::render(f, layout[1], app),
    }

    render_status_bar(f, layout[2], app);

    if app.root_warning_modal {
        render_root_warning(f, f.area());
    }
}

fn render_tab_bar(f: &mut Frame, area: Rect, app: &App) {
    let titles: Vec<Line> = Tab::ALL.iter().map(|t| {
        let style = if *t == app.active_tab {
            Style::default().fg(Color::Cyan).bold().bg(Color::DarkGray)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        Line::from(Span::styled(format!(" {} {} ", t.hotkey(), t.label()), style))
    }).collect();

    f.render_widget(
        Tabs::new(titles)
            .block(Block::default().borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(Span::styled(" benchmon ",
                    Style::default().fg(Color::White).bold())))
            .select(Tab::ALL.iter().position(|t| *t == app.active_tab).unwrap_or(0))
            .highlight_style(Style::default().fg(Color::Cyan).bold()),
        area);
}

fn render_status_bar(f: &mut Frame, area: Rect, app: &App) {
    let ready        = app.verify.as_ref().map(|v| v.all_checks_passed).unwrap_or(false);
    let status_icon  = if ready { "●" } else { "○" };
    let status_color = if ready { Color::Green } else { Color::Yellow };

    let latency = app.latest_snapshot.as_ref()
        .map(|s| format!("{:.1}μs", s.capture_latency_ns as f64 / 1000.0))
        .unwrap_or_else(|| "—".into());

    // ---- Service / log indicator ----
    // When piggybacking, the daemon owns the logger — show a different indicator.
    let monitor_span = if app.piggyback_mode {
        let mode_char = match app.service_mode {
            Some(service::ServiceMode::Persistent) => "P",
            Some(service::ServiceMode::Transient)  => "T",
            None => "?",
        };
        Span::styled(
            format!(" ◉ SVC[{mode_char}] "),
            Style::default().fg(Color::Cyan).bold(),
        )
    } else if app.service_active {
        // Service running but TUI also has own monitor (unusual transient state)
        Span::styled(" ● SVC ", Style::default().fg(Color::Yellow).bold())
    } else if app.logging_active {
        Span::styled(" ◉ LOG ", Style::default().fg(Color::Red).bold())
    } else {
        Span::styled(" ○ ", Style::default().fg(Color::DarkGray))
    };

    let reboot_hint = if app.root_warning_modal {
        Span::styled(" ⚠ NOT ROOT — Enter/Esc to continue  Q to quit ",
            Style::default().fg(Color::Red).bold())
    } else if app.reboot_modal {
        Span::styled(" ⚠ REBOOT PENDING — R to reboot / Esc to dismiss ",
            Style::default().fg(Color::Yellow).bold())
    } else if app.teardown_picker_active {
        Span::styled(" PICKER: ↑↓ select  Enter confirm  i preview  Esc cancel ",
            Style::default().fg(Color::Cyan))
    } else {
        let monitor_hint;
        let hint = if app.active_tab == Tab::Monitor {
            monitor_hint = format!("←→=section [{}]  ↑↓/PgUp/PgDn/wheel=scroll  Home/End",
                app.monitor_section.label());
            monitor_hint.as_str()
        } else if app.setup_editing {
            "Enter=confirm  Esc=cancel"
        } else if app.active_tab == Tab::Terminal && app.terminal_focused {
            "↑↓=history  PgUp/PgDn/wheel=scroll  Enter=run  Esc=unfocus"
        } else if app.active_tab == Tab::Terminal {
            "Enter=focus  ↑↓/PgUp/PgDn/wheel=scroll  Home/End=top/btm"
        } else if app.active_tab == Tab::Logs {
            "↑↓/PgUp/PgDn/wheel=scroll  Home=oldest  End=newest"
        } else if app.piggyback_mode {
            "q:Quit TUI  Tab:Switch  R:Refresh  — daemon keeps running after quit"
        } else {
            "q:Quit  Tab:Switch  R:Refresh  ↑↓:Navigate  Enter:Edit/Toggle"
        };
        Span::styled(hint.to_string(), Style::default().fg(Color::DarkGray))
    };

    let bar = Line::from(vec![
        Span::styled(format!(" {status_icon} "), Style::default().fg(status_color)),
        Span::styled(if ready { "READY" } else { "NOT READY" },
            Style::default().fg(status_color)),
        Span::styled("  │  ", Style::default().fg(Color::DarkGray)),
        Span::styled(format!("Capture: {latency}"),
            Style::default().fg(Color::DarkGray)),
        Span::styled("  │  ", Style::default().fg(Color::DarkGray)),
        Span::styled(format!("Tick #{}", app.tick_count),
            Style::default().fg(Color::DarkGray)),
        Span::styled("  │  ", Style::default().fg(Color::DarkGray)),
        monitor_span,
        Span::styled("  │  ", Style::default().fg(Color::DarkGray)),
        reboot_hint,
    ]);

    f.render_widget(Paragraph::new(bar), area);
}

// ------------------------------------------------------------------ //
//  Root warning modal                                                 //
// ------------------------------------------------------------------ //

fn render_root_warning(f: &mut Frame, area: Rect) {
    let w = 58u16;
    let h = 11u16;
    let x = area.x + area.width.saturating_sub(w) / 2;
    let y = area.y + area.height.saturating_sub(h) / 2;
    let modal = Rect { x, y, width: w.min(area.width), height: h.min(area.height) };

    f.render_widget(Clear, modal);

    let text = vec![
        Line::default(),
        Line::from(Span::styled(
            "  benchmon is NOT running as root.",
            Style::default().fg(Color::White).bold())),
        Line::default(),
        Line::from(Span::styled(
            "  Most features will not work:",
            Style::default().fg(Color::DarkGray))),
        Line::from(Span::styled(
            "  setup, teardown, perf counters, sysfs writes.",
            Style::default().fg(Color::DarkGray))),
        Line::default(),
        Line::from(Span::styled(
            "  Restart with:  sudo ./benchmon",
            Style::default().fg(Color::Yellow).bold())),
        Line::default(),
        Line::from(vec![
            Span::styled("   [ Enter / Esc ] Continue anyway  ",
                Style::default().fg(Color::Rgb(0, 0, 0)).bg(Color::DarkGray).bold()),
            Span::raw("   "),
            Span::styled("  [ Q ] Quit  ",
                Style::default().fg(Color::Rgb(0, 0, 0)).bg(Color::Red).bold()),
        ]),
    ];

    f.render_widget(
        Paragraph::new(text)
            .block(Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Red).bold())
                .title(Span::styled(
                    " ⚠  ROOT REQUIRED ",
                    Style::default().fg(Color::Red).bold()))),
        modal,
    );
}