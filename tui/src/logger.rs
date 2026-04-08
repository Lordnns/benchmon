//! MetricLogger — writes every snapshot to a NDJSON file.
//!
//! Format (one JSON object per line):
//!   {"type":"cpu",     "time":"2026-04-01T06:34:17.123456", "core":<n>, ...}
//!   {"type":"mem",     "time":"...", "total_bytes":<n>, ...}
//!   {"type":"net",     "time":"...", "iface":"<name>", ...}
//!   {"type":"disk",    "time":"...", "dev":"<name>", ...}
//!   {"type":"latency", "time":"...", "capture_ns":<n>}
//!   {"type":"event",   "time":"...", "event":"<type>", "detail":"<msg>"}
//!
//! File location: /tmp/benchmon_metrics_<ISO>.jsonl

use std::fs::{File, OpenOptions};
use std::io::Write;
use crate::ffi::Snapshot;

pub struct MetricLogger {
    file: File,
    pub path: String,
    bytes_written: u64,
}

impl MetricLogger {
    pub fn new() -> Option<Self> {
        let ts = chrono::Local::now().format("%Y-%m-%dT%H-%M-%S").to_string();
        let dir = "/var/log/benchmon";
        std::fs::create_dir_all(dir).ok();
        let path = format!("{}/metrics_{}.jsonl", dir, ts);
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .ok()?;

        Some(MetricLogger { file, path, bytes_written: 0 })
    }

    pub fn bytes_written(&self) -> u64 { self.bytes_written }

    /// Log every subsystem in a snapshot — called every tick.
    pub fn log_snapshot(&mut self, snap: &Snapshot) {
        let t = snap.timestamp_ns;

        self.write_line(&format!(
            r#"{{"type":"latency","time":"{}","capture_ns":{}}}"#,
            ns_to_iso(t), snap.capture_latency_ns
        ));

        for cpu in &snap.cpu {
            self.write_line(&format!(
                r#"{{"type":"cpu","time":"{}","core":{},"freq_mhz":{},"cycles":{},"instructions":{},"cache_misses":{},"usage_pct":{:.4}}}"#,
                ns_to_iso(t), cpu.core_id, cpu.freq_mhz,
                cpu.cycles, cpu.instructions, cpu.cache_misses,
                cpu.usage_pct
            ));
        }

        let m = &snap.mem;
        self.write_line(&format!(
            r#"{{"type":"mem","time":"{}","total_bytes":{},"used_bytes":{},"available_bytes":{},"cache_bytes":{},"swap_used_bytes":{}}}"#,
            ns_to_iso(t), m.total_bytes, m.used_bytes,
            m.available_bytes, m.cache_bytes, m.swap_used_bytes
        ));

        for net in &snap.net {
            self.write_line(&format!(
                r#"{{"type":"net","time":"{}","iface":"{}","rx_bytes":{},"tx_bytes":{},"rx_pkts":{},"tx_pkts":{},"rx_drop":{},"tx_drop":{},"rx_err":{},"tx_err":{}}}"#,
                ns_to_iso(t), net.iface,
                net.rx_bytes, net.tx_bytes,
                net.rx_packets, net.tx_packets,
                net.rx_dropped, net.tx_dropped,
                net.rx_errors, net.tx_errors,
            ));
        }

        for disk in &snap.disk {
            self.write_line(&format!(
                r#"{{"type":"disk","time":"{}","dev":"{}","reads":{},"writes":{},"read_bytes":{},"write_bytes":{},"io_in_progress":{},"io_time_ms":{}}}"#,
                ns_to_iso(t), disk.device,
                disk.reads_completed, disk.writes_completed,
                disk.read_bytes, disk.write_bytes,
                disk.io_in_progress, disk.io_time_ms
            ));
        }
    }

    /// Log a free-form event (setup, teardown, verify, etc.)
    pub fn log_event(&mut self, event_type: &str, detail: &str) {
        let detail = detail.replace('"', "'");
        self.write_line(&format!(
            r#"{{"type":"event","time":"{}","event":"{}","detail":"{}"}}"#,
            now_iso(), event_type, detail
        ));
    }

    fn write_line(&mut self, line: &str) {
        if writeln!(self.file, "{}", line).is_ok() {
            self.bytes_written += line.len() as u64 + 1;
        }
    }
}

/// Convert a nanosecond timestamp to ISO 8601 with microseconds.
fn ns_to_iso(ns: u64) -> String {
    let secs   = (ns / 1_000_000_000) as i64;
    let micros = ((ns % 1_000_000_000) / 1_000) as u32;
    chrono::DateTime::from_timestamp(secs, micros * 1_000)
        .unwrap_or_default()
        .format("%Y-%m-%dT%H:%M:%S%.6f")
        .to_string()
}

/// Current wall-clock time as ISO 8601 with microseconds.
fn now_iso() -> String {
    chrono::Local::now()
        .format("%Y-%m-%dT%H:%M:%S%.6f")
        .to_string()
}

pub fn fmt_size(bytes: u64) -> String {
    if bytes < 1024            { format!("{} B", bytes) }
    else if bytes < 1_048_576  { format!("{:.1} KB", bytes as f64 / 1024.0) }
    else                       { format!("{:.1} MB", bytes as f64 / 1_048_576.0) }
}