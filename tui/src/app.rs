//! Application state shared across all TUI tabs.

use std::collections::VecDeque;
use chrono::Local;
use crate::ffi;
use crate::logger::{self, MetricLogger};
use crate::snapshot_store::{self, ConfigSnapshot};
use crate::service;

const HISTORY_LEN: usize = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Dashboard, Setup, Monitor, Logs, Terminal,
}

impl Tab {
    pub const ALL: [Tab; 5] = [
        Tab::Dashboard, Tab::Setup, Tab::Monitor, Tab::Logs, Tab::Terminal,
    ];
    pub fn label(&self) -> &'static str {
        match self {
            Tab::Dashboard => "Dashboard",
            Tab::Setup     => "Setup",
            Tab::Monitor   => "Monitor",
            Tab::Logs      => "Logs",
            Tab::Terminal  => "Terminal",
        }
    }
    pub fn hotkey(&self) -> &'static str {
        match self {
            Tab::Dashboard => "F1", Tab::Setup => "F2", Tab::Monitor => "F3",
            Tab::Logs => "F4", Tab::Terminal => "F5",
        }
    }
    pub fn next(&self) -> Tab {
        match self {
            Tab::Dashboard => Tab::Setup,   Tab::Setup   => Tab::Monitor,
            Tab::Monitor   => Tab::Logs,    Tab::Logs    => Tab::Terminal,
            Tab::Terminal  => Tab::Dashboard,
        }
    }
    pub fn prev(&self) -> Tab {
        match self {
            Tab::Dashboard => Tab::Terminal, Tab::Setup     => Tab::Dashboard,
            Tab::Monitor   => Tab::Setup,    Tab::Logs      => Tab::Monitor,
            Tab::Terminal  => Tab::Logs,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: String,
    pub level: LogLevel,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel { Info, Warn, Error, Success }

#[derive(Debug, Clone)]
pub struct TimeSeries {
    pub data: VecDeque<f64>,
    #[allow(dead_code)]
    pub label: String,
    pub max: usize,
}
impl TimeSeries {
    pub fn new(label: &str) -> Self {
        Self { data: VecDeque::with_capacity(HISTORY_LEN), label: label.into(), max: HISTORY_LEN }
    }
    pub fn push(&mut self, value: f64) {
        if self.data.len() >= self.max { self.data.pop_front(); }
        self.data.push_back(value);
    }
    pub fn as_slice(&self) -> Vec<f64> { self.data.iter().copied().collect() }
    pub fn last(&self) -> f64 { self.data.back().copied().unwrap_or(0.0) }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MonitorSection { Cpu, Net, Disk }

impl MonitorSection {
    pub fn next(self) -> Self {
        match self { Self::Cpu => Self::Net, Self::Net => Self::Disk, Self::Disk => Self::Cpu }
    }
    pub fn prev(self) -> Self {
        match self { Self::Cpu => Self::Disk, Self::Net => Self::Cpu, Self::Disk => Self::Net }
    }
    pub fn label(self) -> &'static str {
        match self { Self::Cpu => "CPU", Self::Net => "Net", Self::Disk => "Disk" }
    }
}

pub struct App {
    pub active_tab: Tab,
    pub should_quit: bool,
    pub tick_count: u64,

    // ---- Service / piggyback ----------------------------------------
    /// A background daemon is running (transient or persistent).
    pub service_active: bool,
    /// TUI is reading snapshots from the daemon's snap file instead of
    /// running its own monitor.  Closing the TUI does NOT stop the service.
    pub piggyback_mode: bool,
    /// Mode of the active service (if any).
    pub service_mode: Option<service::ServiceMode>,

    // Verification
    pub verify: Option<ffi::VerifyResult>,

    // Setup
    pub setup_config: ffi::SetupConfig,
    pub setup_result: Option<ffi::SetupResult>,
    pub setup_running: bool,
    pub setup_selected_item: usize,
    pub setup_editing: bool,
    pub setup_edit_buffer: String,

    // Root warning modal — shown on startup if not root
    pub root_warning_modal: bool,

    // Reboot modal — shown after GRUB is modified
    pub reboot_modal: bool,
    pub reboot_modal_refresh_on_dismiss: bool,

    // Teardown config picker
    pub teardown_picker_active: bool,
    pub teardown_snapshots: Vec<ConfigSnapshot>,
    pub teardown_picker_idx: usize,
    pub teardown_picker_preview: bool,

    // Monitor (owned — only used when NOT in piggyback mode)
    pub monitor: Option<ffi::Monitor>,
    pub latest_snapshot: Option<ffi::Snapshot>,
    pub cpu_history: Vec<TimeSeries>,
    pub mem_history: TimeSeries,
    pub net_rx_history: Vec<TimeSeries>,
    pub net_tx_history: Vec<TimeSeries>,
    pub disk_io_history: Vec<TimeSeries>,
    pub capture_latency_history: TimeSeries,
    pub monitor_section: MonitorSection,
    pub monitor_cpu_scroll: usize,
    pub monitor_net_scroll: usize,
    pub monitor_disk_scroll: usize,

    // Metric logger (owned — only used when NOT in piggyback mode)
    pub logger: Option<MetricLogger>,
    pub logging_active: bool,

    // TUI logs
    pub logs: Vec<LogEntry>,
    pub log_scroll: usize,

    // Terminal
    pub terminal_input: String,
    pub terminal_output: Vec<String>,
    pub terminal_scroll: usize,
    pub terminal_focused: bool,
    pub cmd_history: Vec<String>,
    pub cmd_history_idx: Option<usize>,
    pub cmd_draft: String,
}

impl App {
    pub fn new() -> Self {
        let mut app = Self {
            active_tab: Tab::Dashboard,
            should_quit: false,
            tick_count: 0,

            service_active: false,
            piggyback_mode: false,
            service_mode: None,

            verify: None,
            setup_config: ffi::SetupConfig::default(),
            setup_result: None,
            setup_running: false,
            setup_selected_item: 0,
            setup_editing: false,
            setup_edit_buffer: String::new(),
            root_warning_modal: false,
            reboot_modal: false,
            reboot_modal_refresh_on_dismiss: false,
            teardown_picker_active: false,
            teardown_snapshots: Vec::new(),
            teardown_picker_idx: 0,
            teardown_picker_preview: false,
            monitor: None,
            latest_snapshot: None,
            cpu_history: Vec::new(),
            mem_history: TimeSeries::new("RAM Usage %"),
            net_rx_history: Vec::new(),
            net_tx_history: Vec::new(),
            disk_io_history: Vec::new(),
            capture_latency_history: TimeSeries::new("Capture Latency μs"),
            monitor_section: MonitorSection::Cpu,
            monitor_cpu_scroll: 0,
            monitor_net_scroll: 0,
            monitor_disk_scroll: 0,
            logger: None,
            logging_active: false,
            logs: Vec::new(),
            log_scroll: 0,
            terminal_input: String::new(),
            terminal_output: vec![
                "benchmon terminal — type commands here".into(),
                "↑↓ browse history  PgUp/PgDn or scroll wheel to scroll output".into(),
                "log start / log stop / log status — metric recording".into(),
                "service start/stop/enable/disable/status — background daemon".into(),
                String::new(),
            ],
            terminal_scroll: 0,
            terminal_focused: false,
            cmd_history: Vec::new(),
            cmd_history_idx: None,
            cmd_draft: String::new(),
        };

        app.log(LogLevel::Info, "benchmon TUI started");

        let euid = std::process::Command::new("id")
            .arg("-u")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();

        if euid != "0" {
            app.root_warning_modal = true;
            app.log(LogLevel::Error,
                "Not running as root — setup/teardown/perf monitoring disabled");
            app.log(LogLevel::Warn, "Restart with: sudo ./benchmon");
        }

        app.refresh_verify();
        // Only start own logging when not piggybacking (service has its own logger)
        if !app.piggyback_mode {
            app.start_logging();
        }
        app
    }

    // ---------------------------------------------------------------- //
    //  Logging helpers                                                  //
    // ---------------------------------------------------------------- //

    pub fn log(&mut self, level: LogLevel, msg: &str) {
        self.logs.push(LogEntry {
            timestamp: Local::now().format("%H:%M:%S%.3f").to_string(),
            level,
            message: msg.to_string(),
        });
        if let Some(ref mut lg) = self.logger {
            let kind = match level {
                LogLevel::Info    => "info",
                LogLevel::Warn    => "warn",
                LogLevel::Error   => "error",
                LogLevel::Success => "success",
            };
            lg.log_event(kind, msg);
        }
    }

    pub fn start_logging(&mut self) {
        if self.logging_active { return; }
        match MetricLogger::new() {
            Some(lg) => {
                let path = lg.path.clone();
                self.logger = Some(lg);
                self.logging_active = true;
                self.log(LogLevel::Success, &format!("Metric log: {}", path));
            }
            None => {
                self.log(LogLevel::Error,
                    "Could not open metric log file in /var/log/benchmon");
            }
        }
    }

    pub fn stop_logging(&mut self) {
        self.logger = None;
        self.logging_active = false;
        self.log(LogLevel::Info, "Metric logging stopped");
    }

    // ---------------------------------------------------------------- //
    //  Verify + monitor reinit                                          //
    // ---------------------------------------------------------------- //

    pub fn refresh_verify(&mut self) {
        self.verify = Some(ffi::run_verify());
        self.log(LogLevel::Info, "System verification refreshed");

        // Reset histories and owned monitor
        self.monitor = None;
        self.latest_snapshot = None;
        self.cpu_history.clear();
        self.net_rx_history.clear();
        self.net_tx_history.clear();
        self.disk_io_history.clear();
        self.capture_latency_history = TimeSeries::new("Capture Latency μs");
        self.mem_history = TimeSeries::new("RAM Usage %");

        // ---- Check for background service ----
        if service::is_running() {
            let pid = service::daemon_pid().unwrap_or(0);
            let mode = service::current_mode();
            self.service_active = true;
            self.piggyback_mode = true;
            self.service_mode = mode;
            self.log(
                LogLevel::Success,
                &format!(
                    "Daemon running (pid={}) [{}] — TUI is piggybacking; \
                     close TUI freely, daemon keeps logging",
                    pid,
                    mode.map(|m| m.label()).unwrap_or("unknown mode"),
                ),
            );
            // Do NOT init our own monitor — we read from the daemon's snap file.
            return;
        }

        // No service running — own monitor as before
        self.service_active = false;
        self.piggyback_mode = false;
        self.service_mode = None;

        if let Some(mon) = ffi::Monitor::new(0x0F) {
            self.log(LogLevel::Success, "Monitor initialized (CPU+MEM+NET+DISK)");
            self.monitor = Some(mon);
        } else {
            self.log(LogLevel::Warn,
                "Monitor init failed — try running as root for perf counters");
        }
    }

    // ---------------------------------------------------------------- //
    //  Root warning modal                                               //
    // ---------------------------------------------------------------- //

    pub fn dismiss_root_warning(&mut self) {
        self.root_warning_modal = false;
    }

    // ---------------------------------------------------------------- //
    //  Reboot modal                                                     //
    // ---------------------------------------------------------------- //

    pub fn show_reboot_modal(&mut self, refresh_on_dismiss: bool) {
        self.reboot_modal = true;
        self.reboot_modal_refresh_on_dismiss = refresh_on_dismiss;
    }

    pub fn dismiss_reboot_modal(&mut self) {
        self.reboot_modal = false;
        if self.reboot_modal_refresh_on_dismiss {
            self.reboot_modal_refresh_on_dismiss = false;
            self.refresh_verify();
        }
    }

    pub fn reboot_now(&mut self) {
        self.reboot_modal = false;
        self.reboot_modal_refresh_on_dismiss = false;
        if let Some(ref mut lg) = self.logger {
            lg.log_event("reboot", "user initiated reboot from modal");
        }
        let _ = std::process::Command::new("reboot").spawn();
        self.should_quit = true;
    }

    // ---------------------------------------------------------------- //
    //  Teardown picker                                                  //
    // ---------------------------------------------------------------- //

    pub fn open_teardown_picker(&mut self) {
        self.teardown_snapshots = snapshot_store::list();
        self.teardown_picker_idx = 0;
        self.teardown_picker_preview = false;

        if self.teardown_snapshots.is_empty() {
            self.log(LogLevel::Warn,
                "No saved config snapshots found — run Apply first to create a restore point");
            return;
        }

        self.teardown_picker_active = true;
    }

    pub fn teardown_picker_up(&mut self) {
        if self.teardown_picker_idx > 0 { self.teardown_picker_idx -= 1; }
    }

    pub fn teardown_picker_down(&mut self) {
        if self.teardown_picker_idx + 1 < self.teardown_snapshots.len() {
            self.teardown_picker_idx += 1;
        }
    }

    pub fn teardown_picker_confirm(&mut self) {
        let idx = self.teardown_picker_idx;
        let snap = self.teardown_snapshots.get(idx).cloned();
        self.teardown_picker_active = false;
        if let Some(ref s) = snap {
            self.log(LogLevel::Info,
                &format!("Restoring config from {}", s.timestamp_str));
        }
        self.run_teardown_inner(snap.as_ref());
    }

    pub fn teardown_picker_cancel(&mut self) {
        self.teardown_picker_active = false;
        self.log(LogLevel::Info, "Teardown cancelled");
    }

    fn run_teardown_inner(&mut self, snap: Option<&ConfigSnapshot>) {
        self.log(LogLevel::Info, "Running teardown...");
        if let Some(ref mut lg) = self.logger {
            lg.log_event("teardown", "started");
        }

        let cleanup_status = ffi::run_teardown(&self.setup_config);
        match cleanup_status {
            ffi::Status::Ok =>
                self.log(LogLevel::Success, "Teardown cleanup done"),
            other =>
                self.log(LogLevel::Error,
                    &format!("Teardown cleanup error: {:?}", other)),
        }

        if let Some(s) = snap {
            if let Some(ref cfg) = s.config {
                self.log(LogLevel::Info, "Re-applying pre-apply state...");
                if let Some(ref mut lg) = self.logger {
                    lg.log_event("teardown", "re-applying preconfig");
                }

                if cfg.isolated_cores.is_empty() {
                    self.log(LogLevel::Info, "GRUB: removing benchmon kernel params...");
                    let _ = std::process::Command::new("sh")
                        .arg("-c")
                        .arg(
                            "sed -i \
                             's/ isolcpus=[^ \"]*//g; \
                              s/ nohz_full=[^ \"]*//g; \
                              s/ rcu_nocbs=[^ \"]*//g; \
                              s/ processor\\.max_cstate=[^ \"]*//g; \
                              s/ nosoftlockup//g' \
                             /etc/default/grub 2>/dev/null \
                             && update-grub 2>/dev/null"
                        )
                        .output();
                }

                let result = ffi::run_setup(cfg);

                if let Some(ref mut lg) = self.logger {
                    lg.log_event("teardown",
                        &format!("preconfig restore status={:?} reboot={}",
                            result.status, result.reboot_required));
                }

                let needs_reboot = result.reboot_required
                    || cfg.isolated_cores.is_empty();

                match result.status {
                    ffi::Status::Ok | ffi::Status::ErrPartial =>
                        self.log(LogLevel::Success, "Pre-apply state restored"),
                    ffi::Status::ErrPerm =>
                        self.log(LogLevel::Error, "Restore failed: need root"),
                    other =>
                        self.log(LogLevel::Error,
                            &format!("Restore finished with: {:?}", other)),
                }

                if needs_reboot {
                    self.log(LogLevel::Warn,
                        "GRUB was modified — reboot required to fully restore kernel params");
                    self.show_reboot_modal(true);
                    return;
                }
            }
        } else {
            self.log(LogLevel::Warn,
                "No preconfig to restore — only cleanup was performed");
        }

        self.refresh_verify();
    }

    // ---------------------------------------------------------------- //
    //  History navigation                                               //
    // ---------------------------------------------------------------- //

    pub fn history_prev(&mut self) {
        if self.cmd_history.is_empty() { return; }
        let idx = match self.cmd_history_idx {
            None    => { self.cmd_draft = self.terminal_input.clone(); self.cmd_history.len() - 1 }
            Some(0) => 0,
            Some(i) => i - 1,
        };
        self.cmd_history_idx = Some(idx);
        self.terminal_input = self.cmd_history[idx].clone();
    }

    pub fn history_next(&mut self) {
        match self.cmd_history_idx {
            None => {}
            Some(i) if i + 1 >= self.cmd_history.len() => {
                self.cmd_history_idx = None;
                self.terminal_input = self.cmd_draft.clone();
            }
            Some(i) => {
                self.cmd_history_idx = Some(i + 1);
                self.terminal_input = self.cmd_history[i + 1].clone();
            }
        }
    }

    // ---------------------------------------------------------------- //
    //  Tick                                                             //
    // ---------------------------------------------------------------- //

    pub fn tick(&mut self) {
        self.tick_count += 1;

        // Get next snapshot — from daemon file (piggyback) or own monitor.
        let snap = if self.piggyback_mode {
            service::read_latest_snapshot()
        } else if let Some(ref mon) = self.monitor {
            mon.snapshot()
        } else {
            None
        };

        if let Some(snap) = snap {
            self.ingest_snapshot(snap);
        }
    }

    /// Update all histories and optionally log — extracted so tick() is clean.
    fn ingest_snapshot(&mut self, snap: ffi::Snapshot) {
        // CPU histories
        while self.cpu_history.len() < snap.cpu.len() {
            let idx = self.cpu_history.len();
            self.cpu_history.push(TimeSeries::new(&format!("Core {idx}")));
        }
        for (i, cpu) in snap.cpu.iter().enumerate() {
            if i < self.cpu_history.len() { self.cpu_history[i].push(cpu.usage_pct); }
        }

        // Memory
        if snap.mem.total_bytes > 0 {
            let pct = snap.mem.used_bytes as f64 / snap.mem.total_bytes as f64 * 100.0;
            self.mem_history.push(pct);
        }

        // Network histories
        while self.net_rx_history.len() < snap.net.len() {
            let idx = self.net_rx_history.len();
            let name = snap.net.get(idx).map(|n| n.iface.as_str()).unwrap_or("?");
            self.net_rx_history.push(TimeSeries::new(&format!("{name} RX")));
            self.net_tx_history.push(TimeSeries::new(&format!("{name} TX")));
        }
        for (i, net) in snap.net.iter().enumerate() {
            if i < self.net_rx_history.len() {
                self.net_rx_history[i].push(net.rx_bytes as f64);
                self.net_tx_history[i].push(net.tx_bytes as f64);
            }
        }

        // Disk histories
        while self.disk_io_history.len() < snap.disk.len() {
            let idx = self.disk_io_history.len();
            let name = snap.disk.get(idx).map(|d| d.device.as_str()).unwrap_or("?");
            self.disk_io_history.push(TimeSeries::new(&format!("{name} IO")));
        }
        for (i, disk) in snap.disk.iter().enumerate() {
            if i < self.disk_io_history.len() {
                self.disk_io_history[i].push(disk.io_in_progress as f64);
            }
        }

        self.capture_latency_history.push(snap.capture_latency_ns as f64 / 1000.0);

        // Only log from the TUI when NOT in piggyback mode.
        // When piggybacking, the daemon owns the metric logger.
        if self.logging_active && !self.piggyback_mode {
            if let Some(ref mut lg) = self.logger {
                lg.log_snapshot(&snap);
            }
        }

        self.latest_snapshot = Some(snap);
    }

    // ---------------------------------------------------------------- //
    //  Terminal commands                                                //
    // ---------------------------------------------------------------- //

    pub fn exec_terminal_command(&mut self) {
        let cmd = self.terminal_input.clone();
        self.terminal_input.clear();
        self.cmd_history_idx = None;
        self.cmd_draft.clear();
        if cmd.is_empty() { return; }

        if self.cmd_history.last().map(|s| s.as_str()) != Some(&cmd) {
            self.cmd_history.push(cmd.clone());
            if self.cmd_history.len() > 500 { self.cmd_history.remove(0); }
        }

        self.terminal_output.push(format!("$ {cmd}"));

        match cmd.as_str() {
            // ---- Verify ----
            "benchmon verify" | "verify" => {
                self.refresh_verify();
                if let Some(ref v) = self.verify {
                    self.terminal_output.push(format!(
                        "CPU: {} | SMT: {} | Isolated: {} | Swap: {} | Bare-metal: {}",
                        v.cpu_model,
                        if v.smt_disabled { "off ✓" } else { "on ✗" },
                        if v.cores_isolated { "yes ✓" } else { "no ✗" },
                        if v.swap_off { "off ✓" } else { "on ✗" },
                        if v.running_bare_metal { "yes ✓" } else { "no ✗" },
                    ));
                    if !v.warnings.is_empty() {
                        for line in v.warnings.lines() {
                            self.terminal_output.push(line.to_string());
                        }
                    }
                }
                return;
            }

            // ---- Metric log ----
            "log start" => {
                if self.piggyback_mode {
                    self.terminal_output.push(
                        "Daemon is already logging — use 'service status' to see log path.".into());
                    return;
                }
                self.start_logging();
                if let Some(ref lg) = self.logger {
                    self.terminal_output.push(format!("Logging to: {}", lg.path));
                }
                return;
            }
            "log stop" => {
                if self.piggyback_mode {
                    self.terminal_output.push(
                        "Logging is owned by the daemon — use 'service stop' to stop it.".into());
                    return;
                }
                self.stop_logging();
                self.terminal_output.push("Metric logging stopped.".into());
                return;
            }
            "log status" => {
                if self.piggyback_mode {
                    self.terminal_output.push(
                        format!("Logging owned by daemon — {}", service::status_string()));
                } else if self.logging_active {
                    if let Some(ref lg) = self.logger {
                        self.terminal_output.push(format!(
                            "Logging ACTIVE → {}  ({})",
                            lg.path, logger::fmt_size(lg.bytes_written())
                        ));
                    }
                } else {
                    self.terminal_output.push(
                        "Logging INACTIVE — type 'log start' to enable.".into());
                }
                return;
            }

            // ---- Service: start ----
            "service start" => {
                if service::is_running() {
                    self.terminal_output.push(format!(
                        "Daemon already running (pid {}).",
                        service::daemon_pid().unwrap_or(0)
                    ));
                    return;
                }
                self.terminal_output.push("Starting transient daemon...".into());
                match service::start() {
                    Ok(()) => {
                        self.terminal_output.push("Daemon started.".into());
                        // Re-check service + switch to piggyback
                        self.refresh_verify();
                        // Stop own logging — daemon has it now
                        if self.piggyback_mode && self.logging_active {
                            self.stop_logging();
                        }
                    }
                    Err(e) => self.terminal_output.push(format!("Error: {e}")),
                }
                return;
            }

            // ---- Service: stop ----
            "service stop" => {
                if !service::is_running() {
                    self.terminal_output.push("Daemon is not running.".into());
                    return;
                }
                // Warn if persistent unit is installed — stop only stops this instance
                if std::path::Path::new(service::UNIT_FILE).exists() {
                    self.terminal_output.push(
                        "Warning: systemd unit is installed — daemon will restart on next boot.".into());
                    self.terminal_output.push(
                        "Use 'service disable' to fully remove the persistent service.".into());
                }
                self.terminal_output.push("Stopping daemon...".into());
                match service::stop() {
                    Ok(()) => {
                        self.terminal_output.push("Daemon stopped.".into());
                        self.service_active = false;
                        self.piggyback_mode = false;
                        self.service_mode   = None;
                        // Re-init own monitor
                        self.refresh_verify();
                        self.start_logging();
                    }
                    Err(e) => self.terminal_output.push(format!("Error: {e}")),
                }
                return;
            }

            // ---- Service: enable ----
            "service enable" => {
                self.terminal_output.push(
                    "Installing systemd unit + starting persistent daemon...".into());
                match service::enable() {
                    Ok(()) => {
                        self.terminal_output.push(
                            "Service enabled and started. Will restart on each boot.".into());
                        self.terminal_output.push(
                            "Runtime setup (veth/IRQ/swap) will be re-applied automatically after every reboot.".into());
                        // Switch to piggyback
                        self.refresh_verify();
                        if self.piggyback_mode && self.logging_active {
                            self.stop_logging();
                        }
                    }
                    Err(e) => self.terminal_output.push(format!("Error: {e}")),
                }
                return;
            }

            // ---- Service: disable ----
            "service disable" => {
                self.terminal_output.push(
                    "Stopping and disabling persistent service...".into());
                match service::disable() {
                    Ok(()) => {
                        self.terminal_output.push("Service disabled and unit removed.".into());
                        self.service_active = false;
                        self.piggyback_mode = false;
                        self.service_mode   = None;
                        self.refresh_verify();
                        self.start_logging();
                    }
                    Err(e) => self.terminal_output.push(format!("Error: {e}")),
                }
                return;
            }

            // ---- Service: status ----
            "service status" => {
                for line in service::status_string().lines() {
                    self.terminal_output.push(line.to_string());
                }
                return;
            }

            // ---- Misc ----
            "clear" => { self.terminal_output.clear(); return; }
            "help"  => {
                for line in [
                    "Built-in commands:",
                    "  verify                   — re-run system verification",
                    "  log start/stop/status    — toggle/check NDJSON metric recording",
                    "  service start            — start background daemon (transient)",
                    "  service stop             — stop daemon (TUI still closes normally)",
                    "  service enable           — install systemd unit (persistent, reboot-safe)",
                    "  service disable          — remove systemd unit + stop daemon",
                    "  service status           — show daemon PID, mode, log path",
                    "  clear / quit             — clear output / exit TUI",
                    "When a service is active, closing the TUI does NOT stop monitoring.",
                    "Only 'service stop' or 'service disable' can stop the daemon.",
                ] { self.terminal_output.push(line.into()); }
                return;
            }
            "quit" | "exit" => { self.should_quit = true; return; }
            _ => {}
        }

        match std::process::Command::new("sh").arg("-c").arg(&cmd).output() {
            Ok(out) => {
                for line in String::from_utf8_lossy(&out.stdout).lines() {
                    self.terminal_output.push(line.to_string());
                }
                for line in String::from_utf8_lossy(&out.stderr).lines() {
                    self.terminal_output.push(format!("ERR: {line}"));
                }
            }
            Err(e) => { self.terminal_output.push(format!("exec error: {e}")); }
        }
    }
}