// Pre-generated bindings for libbenchmon.
//
// This file is a fallback for systems without libclang/bindgen.
// Regenerate with: cd tui && cargo build (requires libclang-dev).
//
// Keep in sync with ../include/benchmon.h

#[allow(non_upper_case_globals, non_camel_case_types, non_snake_case, dead_code)]

use std::os::raw::{c_char, c_double, c_int, c_uint};

// ------------------------------------------------------------------ //
//  Status enum                                                        //
// ------------------------------------------------------------------ //
pub type benchmon_status_t = c_int;

// ------------------------------------------------------------------ //
//  Setup                                                              //
// ------------------------------------------------------------------ //

#[repr(C)]
#[derive(Debug, Default)]
pub struct benchmon_setup_config_t {
    pub isolated_cores: *const c_int,
    pub isolated_cores_count: c_int,
    pub housekeeping_core: c_int,
    pub disable_frequency_boost: c_int,
    pub lock_frequency_mhz: c_int,
    pub disable_smt: c_int,
    pub max_cstate: c_int,
    pub stop_irqbalance: c_int,
    pub disable_swap: c_int,
    pub isolate_multiuser: c_int,
    pub ns_server_name: *const c_char,
    pub ns_client_name: *const c_char,
    pub veth_server_name: *const c_char,
    pub veth_client_name: *const c_char,
    pub server_ip: *const c_char,
    pub client_ip: *const c_char,
    pub netem_delay_ms: c_int,
    pub netem_jitter_ms: c_int,
    pub netem_loss_pct: c_double,
    pub disable_offloading: c_int,
    pub server_cores: [c_char; 64],
    pub client_cores: [c_char; 64],
    pub rt_priority: c_int,
    pub disable_aslr: c_int,
    pub tune_net_buffers: c_int,
    pub drop_caches: c_int,
    pub stop_timesyncd: c_int,
}

#[repr(C)]
#[derive(Debug)]
pub struct benchmon_setup_result_t {
    pub status: benchmon_status_t,
    pub reboot_required: c_int,
    pub message: [c_char; 512],
    pub grub_modified: c_int,
    pub smt_disabled: c_int,
    pub irq_migrated: c_int,
    pub namespaces_created: c_int,
    pub netem_applied: c_int,
    pub offloading_disabled: c_int,
    pub swap_disabled: c_int,
    pub services_stopped: c_int,
    pub frequency_locked: c_int,
    pub sysctl_tuned: c_int,
    pub caches_dropped: c_int,
    pub process_isolation_ready: c_int,
}

// ------------------------------------------------------------------ //
//  Verify                                                             //
// ------------------------------------------------------------------ //

#[repr(C)]
#[derive(Debug)]
pub struct benchmon_verify_result_t {
    pub smt_disabled: c_int,
    pub threads_per_core: c_int,
    pub cores_isolated: c_int,
    pub isolated_core_list: [c_int; 64],
    pub isolated_core_count: c_int,
    pub nohz_full_active: c_int,
    pub frequency_boost_off: c_int,
    pub actual_freq_mhz: c_int,
    pub swap_off: c_int,
    pub total_ram_mb: u64,
    pub available_ram_mb: u64,
    pub ns_server_exists: c_int,
    pub ns_client_exists: c_int,
    pub veth_link_up: c_int,
    pub offloading_disabled: c_int,
    pub netem_active: c_int,
    pub irqbalance_stopped: c_int,
    pub running_bare_metal: c_int,
    pub kernel_version: [c_char; 64],
    pub cpu_model: [c_char; 128],
    pub hypervisor: [c_char; 64],
    pub all_checks_passed: c_int,
    pub warnings: [c_char; 1024],
}

// ------------------------------------------------------------------ //
//  Monitor                                                            //
// ------------------------------------------------------------------ //

#[repr(C)]
pub struct benchmon_monitor {
    _opaque: [u8; 0],
}

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct benchmon_cpu_sample_t {
    pub core_id: c_int,
    pub timestamp_ns: u64,
    pub tsc: u64,
    pub instructions: u64,
    pub cycles: u64,
    pub cache_misses: u64,
    pub freq_mhz: c_uint,
    pub cstate: c_uint,
    pub usage_pct: c_double,
}

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct benchmon_mem_sample_t {
    pub timestamp_ns: u64,
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub used_bytes: u64,
    pub cache_bytes: u64,
    pub swap_used_bytes: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct benchmon_net_sample_t {
    pub timestamp_ns: u64,
    pub iface: [c_char; 16],
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_packets: u64,
    pub tx_packets: u64,
    pub rx_dropped: u64,
    pub tx_dropped: u64,
    pub rx_errors: u64,
    pub tx_errors: u64,
    pub passthru_active: c_int,
}

impl Default for benchmon_net_sample_t {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct benchmon_disk_sample_t {
    pub timestamp_ns: u64,
    pub device: [c_char; 32],
    pub reads_completed: u64,
    pub writes_completed: u64,
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub io_in_progress: u64,
    pub io_time_ms: u64,
}

impl Default for benchmon_disk_sample_t {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

#[repr(C)]
#[derive(Debug)]
pub struct benchmon_snapshot_t {
    pub timestamp_ns: u64,
    pub capture_latency_ns: u64,
    pub cpu: [benchmon_cpu_sample_t; 64],
    pub cpu_count: c_int,
    pub mem: benchmon_mem_sample_t,
    pub net: [benchmon_net_sample_t; 8],
    pub net_count: c_int,
    pub disk: [benchmon_disk_sample_t; 8],
    pub disk_count: c_int,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct benchmon_packet_event_t {
    pub hw_timestamp_ns: u64,
    pub sw_timestamp_ns: u64,
    pub packet_id: c_uint,
    pub packet_size: c_uint,
    pub direction: u8,
    pub protocol: u8,
}

// ------------------------------------------------------------------ //
//  Extern C functions                                                 //
// ------------------------------------------------------------------ //

extern "C" {
    pub fn benchmon_setup(
        cfg: *const benchmon_setup_config_t,
        result: *mut benchmon_setup_result_t,
    ) -> benchmon_status_t;

    pub fn benchmon_teardown(
        cfg: *const benchmon_setup_config_t,
    ) -> benchmon_status_t;

    pub fn benchmon_verify(
        result: *mut benchmon_verify_result_t,
        cfg: *const benchmon_setup_config_t,
    ) -> benchmon_status_t;

    pub fn benchmon_monitor_init(
        flags: c_uint,
        cores: *const c_int,
        core_count: c_int,
    ) -> *mut benchmon_monitor;

    pub fn benchmon_snapshot(
        mon: *mut benchmon_monitor,
        snap: *mut benchmon_snapshot_t,
    ) -> benchmon_status_t;

    pub fn benchmon_monitor_destroy(mon: *mut benchmon_monitor);

    pub fn benchmon_net_passthru_available() -> c_int;
    pub fn benchmon_net_passthru_open(iface: *const c_char) -> c_int;
    pub fn benchmon_net_passthru_read(
        fd: c_int,
        events: *mut benchmon_packet_event_t,
        max: c_int,
    ) -> c_int;
    pub fn benchmon_net_passthru_close(fd: c_int);

    pub fn benchmon_strerror() -> *const c_char;
    pub fn benchmon_report(fd: c_int) -> benchmon_status_t;
    pub fn benchmon_get_launch_prefix(
        cfg: *const benchmon_setup_config_t,
        is_server: c_int,
    ) -> *mut c_char;
}