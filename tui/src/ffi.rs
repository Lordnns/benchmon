//! Safe Rust wrappers around libbenchmon C FFI.

#![allow(non_upper_case_globals, non_camel_case_types, non_snake_case, dead_code)]

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

use std::ffi::CStr;

// ------------------------------------------------------------------ //
//  Status                                                             //
// ------------------------------------------------------------------ //

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ok, ErrPerm, ErrReboot, ErrPartial, ErrIo,
    ErrInval, ErrNotSup, ErrAlready, ErrBusy, Unknown(i32),
}

impl From<benchmon_status_t> for Status {
    fn from(s: benchmon_status_t) -> Self {
        match s {
            0  => Status::Ok,
            -1 => Status::ErrPerm,
            -2 => Status::ErrReboot,
            -3 => Status::ErrPartial,
            -4 => Status::ErrIo,
            -5 => Status::ErrInval,
            -6 => Status::ErrNotSup,
            -7 => Status::ErrAlready,
            -8 => Status::ErrBusy,
            v  => Status::Unknown(v),
        }
    }
}

// ------------------------------------------------------------------ //
//  Setup config                                                       //
// ------------------------------------------------------------------ //

#[derive(Debug, Clone)]
pub struct SetupConfig {
    pub isolated_cores: Vec<i32>,
    pub housekeeping_core: i32,
    pub disable_frequency_boost: bool,
    pub disable_smt: bool,
    pub max_cstate: i32,
    pub stop_irqbalance: bool,
    pub disable_swap: bool,
    pub isolate_multiuser: bool,
    pub ns_server: String,
    pub ns_client: String,
    pub veth_server: String,
    pub veth_client: String,
    pub server_ip: String,
    pub client_ip: String,
    pub netem_delay_ms: i32,
    pub netem_jitter_ms: i32,
    pub netem_loss_pct: f64,
    pub disable_offloading: bool,
}

impl Default for SetupConfig {
    fn default() -> Self {
        Self {
            isolated_cores: vec![2, 3, 4, 5],
            housekeeping_core: 0,
            disable_frequency_boost: true,
            disable_smt: true,
            max_cstate: 0,
            stop_irqbalance: true,
            disable_swap: true,
            isolate_multiuser: false,
            ns_server: "ns-server".into(),
            ns_client: "ns-client".into(),
            veth_server: "veth-s".into(),
            veth_client: "veth-c".into(),
            server_ip: "10.0.0.1/24".into(),
            client_ip: "10.0.0.2/24".into(),
            netem_delay_ms: 25,
            netem_jitter_ms: 5,
            netem_loss_pct: 0.1,
            disable_offloading: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SetupResult {
    pub status: Status,
    pub reboot_required: bool,
    pub message: String,
    pub grub_modified: bool,
    pub smt_disabled: bool,
    pub irq_migrated: bool,
    pub namespaces_created: bool,
    pub netem_applied: bool,
    pub offloading_disabled: bool,
    pub swap_disabled: bool,
    pub services_stopped: bool,
    pub frequency_locked: bool,
}

/// Build a C config struct from a SetupConfig.
/// Empty strings become null pointers so setup.c skips those sections.
fn make_c_cfg(
    cfg: &SetupConfig,
    ns_s: &Option<std::ffi::CString>,
    ns_c: &Option<std::ffi::CString>,
    ve_s: &Option<std::ffi::CString>,
    ve_c: &Option<std::ffi::CString>,
    ip_s: &Option<std::ffi::CString>,
    ip_c: &Option<std::ffi::CString>,
) -> benchmon_setup_config_t {
    benchmon_setup_config_t {
        isolated_cores: if cfg.isolated_cores.is_empty() {
            std::ptr::null()
        } else {
            cfg.isolated_cores.as_ptr()
        },
        isolated_cores_count: cfg.isolated_cores.len() as i32,
        housekeeping_core: cfg.housekeeping_core,
        disable_frequency_boost: cfg.disable_frequency_boost as i32,
        lock_frequency_mhz: 0,
        disable_smt: cfg.disable_smt as i32,
        max_cstate: cfg.max_cstate,
        stop_irqbalance: cfg.stop_irqbalance as i32,
        disable_swap: cfg.disable_swap as i32,
        isolate_multiuser: cfg.isolate_multiuser as i32,
        ns_server_name: ns_s.as_ref().map(|s| s.as_ptr()).unwrap_or(std::ptr::null()),
        ns_client_name: ns_c.as_ref().map(|s| s.as_ptr()).unwrap_or(std::ptr::null()),
        veth_server_name: ve_s.as_ref().map(|s| s.as_ptr()).unwrap_or(std::ptr::null()),
        veth_client_name: ve_c.as_ref().map(|s| s.as_ptr()).unwrap_or(std::ptr::null()),
        server_ip: ip_s.as_ref().map(|s| s.as_ptr()).unwrap_or(std::ptr::null()),
        client_ip: ip_c.as_ref().map(|s| s.as_ptr()).unwrap_or(std::ptr::null()),
        netem_delay_ms: cfg.netem_delay_ms,
        netem_jitter_ms: cfg.netem_jitter_ms,
        netem_loss_pct: cfg.netem_loss_pct,
        disable_offloading: cfg.disable_offloading as i32,
    }
}

fn opt_cstring(s: &str) -> Option<std::ffi::CString> {
    if s.is_empty() { None } else { Some(std::ffi::CString::new(s).unwrap()) }
}

pub fn run_setup(cfg: &SetupConfig) -> SetupResult {
    let ns_s = opt_cstring(&cfg.ns_server);
    let ns_c = opt_cstring(&cfg.ns_client);
    let ve_s = opt_cstring(&cfg.veth_server);
    let ve_c = opt_cstring(&cfg.veth_client);
    let ip_s = opt_cstring(&cfg.server_ip);
    let ip_c = opt_cstring(&cfg.client_ip);

    let c_cfg = make_c_cfg(cfg, &ns_s, &ns_c, &ve_s, &ve_c, &ip_s, &ip_c);

    let mut c_res: benchmon_setup_result_t = unsafe { std::mem::zeroed() };
    unsafe { benchmon_setup(&c_cfg, &mut c_res) };

    let msg = unsafe {
        CStr::from_ptr(c_res.message.as_ptr())
            .to_string_lossy()
            .into_owned()
    };

    SetupResult {
        status: Status::from(c_res.status),
        reboot_required: c_res.reboot_required != 0,
        message: msg,
        grub_modified: c_res.grub_modified != 0,
        smt_disabled: c_res.smt_disabled != 0,
        irq_migrated: c_res.irq_migrated != 0,
        namespaces_created: c_res.namespaces_created != 0,
        netem_applied: c_res.netem_applied != 0,
        offloading_disabled: c_res.offloading_disabled != 0,
        swap_disabled: c_res.swap_disabled != 0,
        services_stopped: c_res.services_stopped != 0,
        frequency_locked: c_res.frequency_locked != 0,
    }
}

pub fn run_teardown(cfg: &SetupConfig) -> Status {
    let ns_s = opt_cstring(&cfg.ns_server);
    let ns_c = opt_cstring(&cfg.ns_client);
    let ve_s = opt_cstring(&cfg.veth_server);
    let ve_c = opt_cstring(&cfg.veth_client);
    let ip_s = opt_cstring(&cfg.server_ip);
    let ip_c = opt_cstring(&cfg.client_ip);

    let c_cfg = make_c_cfg(cfg, &ns_s, &ns_c, &ve_s, &ve_c, &ip_s, &ip_c);

    Status::from(unsafe { benchmon_teardown(&c_cfg) })
}

// ------------------------------------------------------------------ //
//  Verification                                                       //
// ------------------------------------------------------------------ //

#[derive(Debug, Clone, Default)]
pub struct VerifyResult {
    pub smt_disabled: bool,
    pub threads_per_core: i32,
    pub cores_isolated: bool,
    pub isolated_core_list: Vec<i32>,
    pub nohz_full_active: bool,
    pub frequency_boost_off: bool,
    pub actual_freq_mhz: i32,
    pub swap_off: bool,
    pub total_ram_mb: u64,
    pub available_ram_mb: u64,
    pub ns_server_exists: bool,
    pub ns_client_exists: bool,
    pub veth_link_up: bool,
    pub offloading_disabled: bool,
    pub netem_active: bool,
    pub irqbalance_stopped: bool,
    pub running_bare_metal: bool,
    pub kernel_version: String,
    pub cpu_model: String,
    pub hypervisor: String,
    pub all_checks_passed: bool,
    pub warnings: String,
}

pub fn run_verify() -> VerifyResult {
    let mut c_res: benchmon_verify_result_t = unsafe { std::mem::zeroed() };
    unsafe { benchmon_verify(&mut c_res) };

    let mut iso_list = Vec::new();
    for i in 0..c_res.isolated_core_count as usize {
        iso_list.push(c_res.isolated_core_list[i]);
    }

    let str_field = |arr: &[std::os::raw::c_char]| -> String {
        unsafe { CStr::from_ptr(arr.as_ptr()).to_string_lossy().into_owned() }
    };

    VerifyResult {
        smt_disabled: c_res.smt_disabled != 0,
        threads_per_core: c_res.threads_per_core,
        cores_isolated: c_res.cores_isolated != 0,
        isolated_core_list: iso_list,
        nohz_full_active: c_res.nohz_full_active != 0,
        frequency_boost_off: c_res.frequency_boost_off != 0,
        actual_freq_mhz: c_res.actual_freq_mhz,
        swap_off: c_res.swap_off != 0,
        total_ram_mb: c_res.total_ram_mb,
        available_ram_mb: c_res.available_ram_mb,
        ns_server_exists: c_res.ns_server_exists != 0,
        ns_client_exists: c_res.ns_client_exists != 0,
        veth_link_up: c_res.veth_link_up != 0,
        offloading_disabled: c_res.offloading_disabled != 0,
        netem_active: c_res.netem_active != 0,
        irqbalance_stopped: c_res.irqbalance_stopped != 0,
        running_bare_metal: c_res.running_bare_metal != 0,
        kernel_version: str_field(&c_res.kernel_version),
        cpu_model: str_field(&c_res.cpu_model),
        hypervisor: str_field(&c_res.hypervisor),
        all_checks_passed: c_res.all_checks_passed != 0,
        warnings: str_field(&c_res.warnings),
    }
}

// ------------------------------------------------------------------ //
//  Monitor                                                            //
// ------------------------------------------------------------------ //

pub struct Monitor { handle: *mut benchmon_monitor }
unsafe impl Send for Monitor {}

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub timestamp_ns: u64,
    pub capture_latency_ns: u64,
    pub cpu: Vec<CpuSample>,
    pub mem: MemSample,
    pub net: Vec<NetSample>,
    pub disk: Vec<DiskSample>,
}

#[derive(Debug, Clone, Default)]
pub struct CpuSample {
    pub core_id: i32,
    pub freq_mhz: u32,
    pub instructions: u64,
    pub cycles: u64,
    pub cache_misses: u64,
    pub usage_pct: f64,
}

#[derive(Debug, Clone, Default)]
pub struct MemSample {
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub used_bytes: u64,
    pub cache_bytes: u64,
    pub swap_used_bytes: u64,
}

#[derive(Debug, Clone, Default)]
pub struct NetSample {
    pub iface: String,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_packets: u64,
    pub tx_packets: u64,
    pub rx_dropped: u64,
    pub tx_dropped: u64,
    pub rx_errors: u64,
    pub tx_errors: u64,
    pub passthru_active: bool,
}

#[derive(Debug, Clone, Default)]
pub struct DiskSample {
    pub device: String,
    pub reads_completed: u64,
    pub writes_completed: u64,
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub io_in_progress: u64,
    pub io_time_ms: u64,
}

// ------------------------------------------------------------------ //
//  C struct → Rust Snapshot conversion (shared by snapshot() and     //
//  snapshot_from_bytes() so we call benchmon_snapshot() only once)   //
// ------------------------------------------------------------------ //

fn c_str_arr(arr: &[std::os::raw::c_char]) -> String {
    unsafe { CStr::from_ptr(arr.as_ptr()).to_string_lossy().into_owned() }
}

fn c_snap_to_rust(c: &benchmon_snapshot_t) -> Snapshot {
    let cpu: Vec<CpuSample> = (0..c.cpu_count as usize).map(|i| {
        let s = &c.cpu[i];
        CpuSample {
            core_id: s.core_id, freq_mhz: s.freq_mhz,
            instructions: s.instructions, cycles: s.cycles,
            cache_misses: s.cache_misses, usage_pct: s.usage_pct,
        }
    }).collect();

    let net: Vec<NetSample> = (0..c.net_count as usize).map(|i| {
        let n = &c.net[i];
        NetSample {
            iface: c_str_arr(&n.iface),
            rx_bytes: n.rx_bytes, tx_bytes: n.tx_bytes,
            rx_packets: n.rx_packets, tx_packets: n.tx_packets,
            rx_dropped: n.rx_dropped, tx_dropped: n.tx_dropped,
            rx_errors: n.rx_errors, tx_errors: n.tx_errors,
            passthru_active: n.passthru_active != 0,
        }
    }).collect();

    let disk: Vec<DiskSample> = (0..c.disk_count as usize).map(|i| {
        let d = &c.disk[i];
        DiskSample {
            device: c_str_arr(&d.device),
            reads_completed: d.reads_completed,
            writes_completed: d.writes_completed,
            read_bytes: d.read_bytes, write_bytes: d.write_bytes,
            io_in_progress: d.io_in_progress, io_time_ms: d.io_time_ms,
        }
    }).collect();

    Snapshot {
        timestamp_ns: c.timestamp_ns,
        capture_latency_ns: c.capture_latency_ns,
        cpu,
        mem: MemSample {
            total_bytes: c.mem.total_bytes,
            available_bytes: c.mem.available_bytes,
            used_bytes: c.mem.used_bytes,
            cache_bytes: c.mem.cache_bytes,
            swap_used_bytes: c.mem.swap_used_bytes,
        },
        net, disk,
    }
}

impl Monitor {
    pub fn new(flags: u32) -> Option<Self> {
        let handle = unsafe { benchmon_monitor_init(flags, std::ptr::null(), 0) };
        if handle.is_null() { None } else { Some(Monitor { handle }) }
    }

    pub fn new_with_cores(flags: u32, cores: &[i32]) -> Option<Self> {
        let handle = unsafe {
            benchmon_monitor_init(flags, cores.as_ptr(), cores.len() as i32)
        };
        if handle.is_null() { None } else { Some(Monitor { handle }) }
    }

    /// Take one snapshot, return the Rust struct.
    pub fn snapshot(&self) -> Option<Snapshot> {
        let mut c: benchmon_snapshot_t = unsafe { std::mem::zeroed() };
        if unsafe { benchmon_snapshot(self.handle, &mut c) } != 0 { return None; }
        Some(c_snap_to_rust(&c))
    }

    /// Take one snapshot, returning BOTH the Rust struct and the raw C struct
    /// bytes for IPC via the snap file.  Single FFI call — no double-sampling.
    pub fn snapshot_and_raw(&self) -> Option<(Snapshot, Vec<u8>)> {
        let mut c: benchmon_snapshot_t = unsafe { std::mem::zeroed() };
        if unsafe { benchmon_snapshot(self.handle, &mut c) } != 0 { return None; }

        // Safety: benchmon_snapshot_t is repr(C), pod, no pointers.
        let bytes = unsafe {
            std::slice::from_raw_parts(
                &c as *const benchmon_snapshot_t as *const u8,
                std::mem::size_of::<benchmon_snapshot_t>(),
            ).to_vec()
        };

        Some((c_snap_to_rust(&c), bytes))
    }

    /// Deserialize a Snapshot from raw C struct bytes written by the daemon.
    /// Used by the TUI in piggyback mode to read from the shared snap file.
    pub fn snapshot_from_bytes(bytes: &[u8]) -> Option<Snapshot> {
        if bytes.len() < std::mem::size_of::<benchmon_snapshot_t>() {
            return None;
        }
        // Safety: bytes came from snapshot_and_raw() in the same binary,
        // so layout is identical.  read_unaligned handles any alignment gap.
        let c: benchmon_snapshot_t = unsafe {
            std::ptr::read_unaligned(bytes.as_ptr() as *const benchmon_snapshot_t)
        };
        Some(c_snap_to_rust(&c))
    }
}

impl Drop for Monitor {
    fn drop(&mut self) { unsafe { benchmon_monitor_destroy(self.handle) }; }
}