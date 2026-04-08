//! Background service management — transient daemon and systemd integration.
//!
//! Two service modes:
//!   Transient  — daemon runs until explicitly stopped; does NOT survive reboot.
//!   Persistent — systemd unit installed; re-applies runtime setup on every boot
//!                and restarts the daemon automatically.
//!
//! IPC: daemon writes each snapshot as raw `benchmon_snapshot_t` bytes to
//!   /var/run/benchmon/snap.bin  (atomic rename from snap.tmp)
//! TUI reads that file every tick when in piggyback mode.

use std::fs;
use std::path::Path;
use std::time::Duration;

use crate::ffi;
use crate::snapshot_store;
use crate::logger::MetricLogger;

// ------------------------------------------------------------------ //
//  Well-known paths                                                   //
// ------------------------------------------------------------------ //

pub const RUN_DIR:   &str = "/var/run/benchmon";
pub const PID_FILE:  &str = "/var/run/benchmon/benchmon.pid";
pub const SNAP_FILE: &str = "/var/run/benchmon/snap.bin";
pub const SNAP_TMP:  &str = "/var/run/benchmon/snap.tmp";
pub const MODE_FILE: &str = "/var/run/benchmon/service.mode";
pub const UNIT_FILE: &str = "/etc/systemd/system/benchmon.service";

// ------------------------------------------------------------------ //
//  Service mode                                                       //
// ------------------------------------------------------------------ //

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceMode { Transient, Persistent }

impl ServiceMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Transient  => "transient",
            Self::Persistent => "persistent",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Transient  => "transient (no reboot persistence)",
            Self::Persistent => "persistent (systemd, restarts on boot)",
        }
    }
}

// ------------------------------------------------------------------ //
//  Daemon detection                                                   //
// ------------------------------------------------------------------ //

/// Returns the PID of the running daemon, or None if not alive.
/// Cleans up a stale PID file automatically.
pub fn daemon_pid() -> Option<u32> {
    let content = fs::read_to_string(PID_FILE).ok()?;
    let pid: u32 = content.trim().parse().ok()?;
    if Path::new(&format!("/proc/{}", pid)).exists() {
        Some(pid)
    } else {
        // Stale PID file — clean up
        let _ = fs::remove_file(PID_FILE);
        None
    }
}

pub fn is_running() -> bool { daemon_pid().is_some() }

pub fn current_mode() -> Option<ServiceMode> {
    match fs::read_to_string(MODE_FILE).ok()?.trim() {
        "transient"  => Some(ServiceMode::Transient),
        "persistent" => Some(ServiceMode::Persistent),
        _ => None,
    }
}

// ------------------------------------------------------------------ //
//  Read latest snapshot (TUI piggyback)                              //
// ------------------------------------------------------------------ //

/// Read the latest snapshot written by the daemon.
/// Returns None if no daemon is running or file is stale/corrupt.
pub fn read_latest_snapshot() -> Option<ffi::Snapshot> {
    // Reject if file is older than 2s (daemon stuck / dead)
    let age_ok = fs::metadata(SNAP_FILE)
        .and_then(|m| m.modified())
        .and_then(|t| std::time::SystemTime::now().duration_since(t).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::Other, e)
        }))
        .map(|d| d.as_millis() < 2000)
        .unwrap_or(false);

    if !age_ok { return None; }

    let bytes = fs::read(SNAP_FILE).ok()?;
    ffi::Monitor::snapshot_from_bytes(&bytes)
}

// ------------------------------------------------------------------ //
//  start / stop                                                       //
// ------------------------------------------------------------------ //

/// Spawn a transient background daemon (no systemd, no reboot persistence).
pub fn start() -> Result<(), String> {
    if is_running() {
        return Err(format!(
            "benchmon daemon already running (pid {})",
            daemon_pid().unwrap_or(0)
        ));
    }

    let exe = std::env::current_exe()
        .map_err(|e| format!("Cannot determine own path: {e}"))?;

    std::process::Command::new(&exe)
        .args(["--daemon", "transient"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("Failed to spawn daemon: {e}"))?;

    // Wait up to 2s for PID file
    for _ in 0..20 {
        std::thread::sleep(Duration::from_millis(100));
        if is_running() { return Ok(()); }
    }
    Err("Daemon did not write PID file within 2s — check that you are root".into())
}

/// Send SIGTERM to the running daemon and wait for it to exit.
pub fn stop() -> Result<(), String> {
    let pid = daemon_pid()
        .ok_or_else(|| "benchmon daemon is not running".to_string())?;

    // SIGTERM via kill(1) — avoids libc dependency
    std::process::Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .status()
        .map_err(|e| format!("kill -TERM failed: {e}"))?;

    for _ in 0..30 {
        std::thread::sleep(Duration::from_millis(100));
        if !is_running() {
            let _ = fs::remove_file(SNAP_FILE);
            return Ok(());
        }
    }

    // Last resort: SIGKILL
    let _ = std::process::Command::new("kill")
        .args(["-KILL", &pid.to_string()])
        .status();
    let _ = fs::remove_file(PID_FILE);
    let _ = fs::remove_file(SNAP_FILE);
    Ok(())
}

// ------------------------------------------------------------------ //
//  enable / disable (systemd)                                         //
// ------------------------------------------------------------------ //

/// Install the systemd unit and start the persistent daemon.
/// Requires root.
pub fn enable() -> Result<(), String> {
    require_root("service enable")?;

    let exe = std::env::current_exe()
        .map_err(|e| format!("Cannot determine own path: {e}"))?;

    let unit_content = format!(
        "[Unit]\n\
         Description=benchmon continuous monitoring and logging service\n\
         Documentation=https://github.com/your-org/benchmon\n\
         After=network.target\n\
         DefaultDependencies=no\n\
         Before=shutdown.target\n\
         \n\
         [Service]\n\
         Type=simple\n\
         ExecStart={exe} --daemon persistent\n\
         Restart=always\n\
         RestartSec=5\n\
         TimeoutStopSec=10\n\
         StandardOutput=journal\n\
         StandardError=journal\n\
         SyslogIdentifier=benchmon\n\
         \n\
         [Install]\n\
         WantedBy=multi-user.target\n",
        exe = exe.display(),
    );

    fs::write(UNIT_FILE, &unit_content)
        .map_err(|e| format!("Cannot write {UNIT_FILE}: {e}"))?;

    run_cmd("systemctl", &["daemon-reload"],
            "systemctl daemon-reload failed")?;
    run_cmd("systemctl", &["enable", "benchmon.service"],
            "systemctl enable failed")?;
    run_cmd("systemctl", &["start", "benchmon.service"],
            "systemctl start failed")?;

    // Wait briefly; systemd may take a moment
    for _ in 0..20 {
        std::thread::sleep(Duration::from_millis(150));
        if is_running() { return Ok(()); }
    }

    // Even if PID file not yet seen, the unit is installed — that's success.
    Ok(())
}

/// Stop, disable, and remove the systemd unit.
/// Requires root.
pub fn disable() -> Result<(), String> {
    require_root("service disable")?;

    // Stop — ignore errors (might not be running)
    let _ = std::process::Command::new("systemctl")
        .args(["stop", "benchmon.service"])
        .status();

    let _ = std::process::Command::new("systemctl")
        .args(["disable", "benchmon.service"])
        .status();

    let _ = fs::remove_file(UNIT_FILE);
    let _ = fs::remove_file(PID_FILE);
    let _ = fs::remove_file(SNAP_FILE);
    let _ = fs::remove_file(MODE_FILE);

    run_cmd("systemctl", &["daemon-reload"],
            "systemctl daemon-reload failed")?;
    Ok(())
}

// ------------------------------------------------------------------ //
//  status                                                             //
// ------------------------------------------------------------------ //

/// Human-readable status string (printed to terminal or TUI output).
pub fn status_string() -> String {
    let unit_installed = Path::new(UNIT_FILE).exists();

    match daemon_pid() {
        None => {
            if unit_installed {
                "Service: unit installed but daemon NOT running\n  \
                 Hint: systemctl start benchmon.service"
                    .to_string()
            } else {
                "Service: NOT running (no unit installed)\n  \
                 Hint: 'service start' or 'service enable'"
                    .to_string()
            }
        }
        Some(pid) => {
            let mode_str = current_mode()
                .map(|m| m.label())
                .unwrap_or("unknown mode");

            let snap_age = snap_age_ms()
                .map(|ms| format!("{}ms ago", ms))
                .unwrap_or_else(|| "no snapshot yet".into());

            let log_path = latest_log_path()
                .unwrap_or_else(|| "none".into());

            format!(
                "Service: RUNNING  pid={}  mode={}\n  \
                 systemd unit: {}\n  \
                 last snapshot: {}\n  \
                 log file: {}",
                pid,
                mode_str,
                if unit_installed { "installed" } else { "not installed" },
                snap_age,
                log_path,
            )
        }
    }
}

// ------------------------------------------------------------------ //
//  Daemon main loop                                                   //
// ------------------------------------------------------------------ //

/// The daemon entry point — blocks until killed via SIGTERM.
///
/// Called from `main()` when argv contains `--daemon`.
pub fn run_daemon(mode: ServiceMode) {
    //eprintln!("benchmon daemon: waiting 10s for system to settle...");
    //std::thread::sleep(Duration::from_secs(10));

    if let Err(e) = fs::create_dir_all(RUN_DIR) {
        eprintln!("benchmon daemon: cannot create {RUN_DIR}: {e}");
        return;
    }

    // Write PID file so TUI can detect us
    let pid = std::process::id();
    if let Err(e) = fs::write(PID_FILE, pid.to_string()) {
        eprintln!("benchmon daemon: cannot write PID file: {e}");
        return;
    }
    let _ = fs::write(MODE_FILE, mode.as_str());

    eprintln!("benchmon daemon: starting (pid={pid}, mode={})", mode.as_str());

    // ---- Re-apply runtime setup ----
    // For persistent mode this is critical: veth, swap, IRQ affinity,
    // and frequency governor do NOT survive a reboot, so we rebuild them.
    // The call is idempotent — safe to run even if already configured.
    let cfg = snapshot_store::load_active()
        .unwrap_or_else(|| {
        eprintln!("benchmon daemon: no active_config.json found — using defaults");
        ffi::SetupConfig::default()
    });

    eprintln!(
        "benchmon daemon: config loaded — isolcpus={:?}  smt={}  swap={}  ns={}/{}",
        cfg.isolated_cores, cfg.disable_smt, cfg.disable_swap,
        cfg.ns_server, cfg.ns_client
    );

    let setup = ffi::run_setup(&cfg);
    eprintln!("benchmon daemon: setup status={:?} reboot={}", setup.status, setup.reboot_required);
    eprintln!("benchmon daemon: setup message: {}", setup.message);

    // ---- Monitor init ----
    let mon = match ffi::Monitor::new(0x0F) {
        Some(m) => {
            eprintln!("benchmon daemon: monitor initialized");
            m
        }
        None => {
            eprintln!(
                "benchmon daemon: monitor init failed — \
                 are you running as root?  Exiting."
            );
            let _ = fs::remove_file(PID_FILE);
            return;
        }
    };

    // ---- Metric logger ----
    let mut logger = MetricLogger::new();
    match logger {
        Some(ref lg) => eprintln!("benchmon daemon: logging to {}", lg.path),
        None         => eprintln!("benchmon daemon: metric log unavailable"),
    }

    // ---- Main loop ----
    loop {
        if let Some((snap, raw_bytes)) = mon.snapshot_and_raw() {
            // Atomic write: write to tmp then rename so TUI never reads
            // a partial struct.
            if fs::write(SNAP_TMP, &raw_bytes).is_ok() {
                let _ = fs::rename(SNAP_TMP, SNAP_FILE);
            }
            if let Some(ref mut lg) = logger {
                lg.log_snapshot(&snap);
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    // Process is killed by SIGTERM (default Rust action = terminate).
    // PID file remains but daemon_pid() handles stale files via /proc check.
}

// ------------------------------------------------------------------ //
//  Helpers                                                            //
// ------------------------------------------------------------------ //

fn require_root(ctx: &str) -> Result<(), String> {
    let uid = std::process::Command::new("id")
        .arg("-u")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    if uid == "0" { Ok(()) } else {
        Err(format!("{ctx} requires root — rerun with sudo"))
    }
}

fn run_cmd(cmd: &str, args: &[&str], err_msg: &str) -> Result<(), String> {
    std::process::Command::new(cmd)
        .args(args)
        .status()
        .map_err(|e| format!("{err_msg}: {e}"))?;
    Ok(())
}

fn snap_age_ms() -> Option<u64> {
    let meta  = fs::metadata(SNAP_FILE).ok()?;
    let mtime = meta.modified().ok()?;
    let age   = std::time::SystemTime::now().duration_since(mtime).ok()?;
    Some(age.as_millis() as u64)
}

fn latest_log_path() -> Option<String> {
    let mut entries: Vec<_> = fs::read_dir("/var/log/benchmon")
        .ok()?
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".jsonl"))
        .collect();
    entries.sort_by_key(|e| {
        e.metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
    });
    entries.last().map(|e| e.path().display().to_string())
}