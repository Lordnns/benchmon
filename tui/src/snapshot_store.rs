//! Config snapshot store — saves a JSON copy of SetupConfig to /tmp before
//! every Apply, so Teardown can restore any previous state.
//!
//! Directory layout:
//!   /tmp/benchmon_cfg_<unix_ns>/
//!       config.json

use std::fs;
use chrono::{DateTime, Utc, TimeZone};
use crate::ffi::SetupConfig;

const SNAP_DIR: &str = "/var/lib/benchmon/snapshots";
pub const ACTIVE_CONFIG: &str = "/var/lib/benchmon/active_config.json";

#[derive(Debug, Clone)]
pub struct ConfigSnapshot {
    /// Unix nanoseconds — used for sorting and directory name.
    pub timestamp_ns: u64,
    /// Human-readable timestamp string.
    pub timestamp_str: String,
    /// Full path to config.json.
    pub path: String,
    /// One-line summary of key settings.
    pub preview: String,
    /// Full config for preview bubble.
    pub config: Option<SetupConfig>,
}

pub fn save_active(cfg: &SetupConfig) -> Option<String> {
    fs::create_dir_all("/var/lib/benchmon").ok()?;
    let json = to_json(cfg);
    fs::write(ACTIVE_CONFIG, &json).ok()?;
    Some(ACTIVE_CONFIG.to_string())
}

pub fn load_active() -> Option<SetupConfig> {
    let content = fs::read_to_string(ACTIVE_CONFIG).ok()?;
    from_json(&content)
}

// ------------------------------------------------------------------ //
//  Save                                                               //
// ------------------------------------------------------------------ //

/// Serialize current config and write to /tmp/benchmon_cfg_<ns>/config.json.
/// Returns the path written, or None on error.
pub fn save(cfg: &SetupConfig, label: &str) -> Option<String> {
    
    let ts = chrono::Local::now().format("%Y-%m-%dT%H-%M-%S").to_string();

    let dir = format!("{}/{}", SNAP_DIR, ts);
    fs::create_dir_all(&dir).ok()?;

    let json = to_json(cfg);
    let path = format!("{}/{}.json", dir, label);
    fs::write(&path, &json).ok()?;
    Some(path)
}

// ------------------------------------------------------------------ //
//  List                                                               //
// ------------------------------------------------------------------ //

/// Scan /tmp for benchmon_cfg_* directories and return sorted snapshot list
/// (newest first).
pub fn list() -> Vec<ConfigSnapshot> {
    let mut out = Vec::new();

    let entries = match fs::read_dir(SNAP_DIR) {
        Ok(e) => e,
        Err(_) => return out,
    };

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();

        let ts_ns = chrono::NaiveDateTime::parse_from_str(&name, "%Y-%m-%dT%H-%M-%S")
            .map(|dt| dt.and_utc().timestamp() as u64 * 1_000_000_000)
            .unwrap_or(0);

        if ts_ns == 0 { continue; }

        let ts_secs  = ts_ns / 1_000_000_000;

        for filename in ["preconfig.json", "config.json"] {
            let path = format!("{}/{}/{}", SNAP_DIR, name, filename);
            if !std::path::Path::new(&path).exists() { continue; }

            let content = fs::read_to_string(&path).unwrap_or_default();
            let config  = from_json(&content);
            let preview = make_preview(config.as_ref());
            let label   = if filename == "preconfig.json" { "PRE" } else { "CFG" };
            let ts_human = format!("{} [{}]", fmt_ts(ts_secs), label);

            out.push(ConfigSnapshot {
                timestamp_ns: ts_ns,
                timestamp_str: ts_human,
                path,
                preview,
                config,
            });
        }
    }
    out.sort_by(|a, b| b.timestamp_ns.cmp(&a.timestamp_ns));
    out
}

// ------------------------------------------------------------------ //
//  Load                                                               //
// ------------------------------------------------------------------ //

pub fn load(path: &str) -> Option<SetupConfig> {
    let content = fs::read_to_string(path).ok()?;
    from_json(&content)
}

// ------------------------------------------------------------------ //
//  JSON helpers (no serde — avoids adding a dependency)              //
// ------------------------------------------------------------------ //

fn to_json(c: &SetupConfig) -> String {
    format!(
        "{{\n\
          \"isolated_cores\": [{cores}],\n\
          \"housekeeping_core\": {hk},\n\
          \"disable_smt\": {smt},\n\
          \"disable_frequency_boost\": {boost},\n\
          \"max_cstate\": {cstate},\n\
          \"stop_irqbalance\": {irq},\n\
          \"disable_swap\": {swap},\n\
          \"isolate_multiuser\": {muser},\n\
          \"ns_server\": \"{ns_s}\",\n\
          \"ns_client\": \"{ns_c}\",\n\
          \"veth_server\": \"{ve_s}\",\n\
          \"veth_client\": \"{ve_c}\",\n\
          \"server_ip\": \"{ip_s}\",\n\
          \"client_ip\": \"{ip_c}\",\n\
          \"netem_delay_ms\": {delay},\n\
          \"netem_jitter_ms\": {jitter},\n\
          \"netem_loss_pct\": {loss},\n\
          \"disable_offloading\": {offload}\n\
        }}",
        cores   = c.isolated_cores.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(", "),
        hk      = c.housekeeping_core,
        smt     = c.disable_smt,
        boost   = c.disable_frequency_boost,
        cstate  = c.max_cstate,
        irq     = c.stop_irqbalance,
        swap    = c.disable_swap,
        muser   = c.isolate_multiuser,
        ns_s    = c.ns_server,
        ns_c    = c.ns_client,
        ve_s    = c.veth_server,
        ve_c    = c.veth_client,
        ip_s    = c.server_ip,
        ip_c    = c.client_ip,
        delay   = c.netem_delay_ms,
        jitter  = c.netem_jitter_ms,
        loss    = c.netem_loss_pct,
        offload = c.disable_offloading,
    )
}

fn from_json(json: &str) -> Option<SetupConfig> {
    let mut c = SetupConfig::default();

    if let Some(v) = json_array(json, "isolated_cores") {
        c.isolated_cores = v.split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();
    }
    if let Some(v) = json_int(json, "housekeeping_core")     { c.housekeeping_core        = v as i32; }
    if let Some(v) = json_bool(json, "disable_smt")          { c.disable_smt               = v; }
    if let Some(v) = json_bool(json, "disable_frequency_boost") { c.disable_frequency_boost = v; }
    if let Some(v) = json_int(json, "max_cstate")             { c.max_cstate               = v as i32; }
    if let Some(v) = json_bool(json, "stop_irqbalance")       { c.stop_irqbalance          = v; }
    if let Some(v) = json_bool(json, "disable_swap")          { c.disable_swap             = v; }
    if let Some(v) = json_bool(json, "isolate_multiuser")     { c.isolate_multiuser        = v; }
    if let Some(v) = json_str(json, "ns_server")              { c.ns_server                = v; }
    if let Some(v) = json_str(json, "ns_client")              { c.ns_client                = v; }
    if let Some(v) = json_str(json, "veth_server")            { c.veth_server              = v; }
    if let Some(v) = json_str(json, "veth_client")            { c.veth_client              = v; }
    if let Some(v) = json_str(json, "server_ip")              { c.server_ip               = v; }
    if let Some(v) = json_str(json, "client_ip")              { c.client_ip               = v; }
    if let Some(v) = json_int(json, "netem_delay_ms")         { c.netem_delay_ms           = v as i32; }
    if let Some(v) = json_int(json, "netem_jitter_ms")        { c.netem_jitter_ms          = v as i32; }
    if let Some(v) = json_float(json, "netem_loss_pct")       { c.netem_loss_pct           = v; }
    if let Some(v) = json_bool(json, "disable_offloading")    { c.disable_offloading       = v; }

    Some(c)
}

// ------------------------------------------------------------------ //
//  Minimal JSON field extractors                                      //
// ------------------------------------------------------------------ //

fn json_array(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\":", key);
    let start  = json.find(&needle)? + needle.len();
    let rest   = json[start..].trim_start();
    if !rest.starts_with('[') { return None; }
    let end = rest.find(']')?;
    Some(rest[1..end].to_string())
}

fn json_int(json: &str, key: &str) -> Option<i64> {
    let needle = format!("\"{}\":", key);
    let start  = json.find(&needle)? + needle.len();
    let rest   = json[start..].trim_start();
    let end    = rest.find(|c: char| !c.is_ascii_digit() && c != '-').unwrap_or(rest.len());
    rest[..end].trim().parse().ok()
}

fn json_float(json: &str, key: &str) -> Option<f64> {
    let needle = format!("\"{}\":", key);
    let start  = json.find(&needle)? + needle.len();
    let rest   = json[start..].trim_start();
    let end    = rest.find(|c: char| !c.is_ascii_digit() && c != '.' && c != '-')
                     .unwrap_or(rest.len());
    rest[..end].trim().parse().ok()
}

fn json_bool(json: &str, key: &str) -> Option<bool> {
    let needle = format!("\"{}\":", key);
    let start  = json.find(&needle)? + needle.len();
    let rest   = json[start..].trim_start();
    if rest.starts_with("true")  { Some(true)  }
    else if rest.starts_with("false") { Some(false) }
    else { None }
}

fn json_str(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\":", key);
    let start  = json.find(&needle)? + needle.len();
    let rest   = json[start..].trim_start();
    if !rest.starts_with('"') { return None; }
    let inner = &rest[1..];
    let end   = inner.find('"')?;
    Some(inner[..end].to_string())
}

// ------------------------------------------------------------------ //
//  Display helpers                                                    //
// ------------------------------------------------------------------ //

fn make_preview(cfg: Option<&SetupConfig>) -> String {
    match cfg {
        None => "  (unreadable)".to_string(),
        Some(c) => format!(
            "isolcpus=[{}]  smt={}  swap={}  netem={}ms±{}ms",
            c.isolated_cores.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(","),
            if c.disable_smt { "off" } else { "on" },
            if c.disable_swap { "off" } else { "on" },
            c.netem_delay_ms,
            c.netem_jitter_ms,
        ),
    }
}

fn fmt_ts(secs: u64) -> String {
    let dt: DateTime<Utc> = match Utc.timestamp_opt(secs as i64, 0) {
        chrono::LocalResult::Single(d) => d,
        _ => return format!("{}s", secs),
    };
    dt.format("%Y-%m-%d %H:%M:%S UTC").to_string()
}

/// Read the current live system state into a SetupConfig so it can be
/// saved as a restore point before Apply runs.
pub fn capture_current_state() -> SetupConfig {
    let mut cfg = SetupConfig::default();

    // SMT
    let smt = std::fs::read_to_string("/sys/devices/system/cpu/smt/active")
        .unwrap_or_default();
    cfg.disable_smt = smt.trim() == "0";

    // Frequency boost (AMD)
    let boost = std::fs::read_to_string("/sys/devices/system/cpu/cpufreq/boost")
        .unwrap_or("1".into());
    cfg.disable_frequency_boost = boost.trim() == "0";

    // Isolated cores from /proc/cmdline
    let cmdline = std::fs::read_to_string("/proc/cmdline").unwrap_or_default();
    if let Some(iso) = cmdline.split_whitespace()
        .find(|p| p.starts_with("isolcpus="))
        .map(|p| p.trim_start_matches("isolcpus="))
    {
        cfg.isolated_cores = iso.split(',')
            .filter_map(|s| s.parse().ok())
            .collect();
    } else {
        cfg.isolated_cores = vec![];
    }

    // Swap
    let swap = std::fs::read_to_string("/proc/swaps").unwrap_or_default();
    let lines: Vec<&str> = swap.lines().collect();
    cfg.disable_swap = lines.len() <= 1; // only header = swap off

    // Namespaces
    let ns_out = std::process::Command::new("ip")
        .args(["netns", "list"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    cfg.ns_server = if ns_out.contains("ns-server") {
        "ns-server".into()
    } else { String::new() };
    cfg.ns_client = if ns_out.contains("ns-client") {
        "ns-client".into()
    } else { String::new() };

    cfg
}
