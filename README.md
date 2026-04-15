# benchmon

**Benchmark Monitor Library + TUI** — A low-latency, minimally-intrusive tool
for setting up and monitoring latency-sensitive protocol benchmarks on Linux.

Built for the paper: *"Benchmarking Latency-Sensitive Network Protocols:
A Reproducible Methodology for Sub-Millisecond Measurements on Linux"*

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    benchmon TUI (Rust)                  │
│  ┌───────┐ ┌──────┐ ┌────────┐ ┌─────┐ ┌──────────┐     │
│  │Dashbrd│ │Setup │ │Monitor │ │Logs │ │Terminal  │     │
│  └───┬───┘ └──┬───┘ └───┬────┘ └──┬──┘ └────┬─────┘     │
│      │        │         │         │          │          │
│      └────────┴─────────┴─────────┴──────────┘          │
│                         │ FFI (bindgen)                 │
├─────────────────────────┼───────────────────────────────┤
│              libbenchmon.so (C)                         │
│  ┌────────┐ ┌─────────┐ ┌────────┐ ┌────────────────┐   │
│  │ setup  │ │ monitor │ │ verify │ │ net passthru   │   │
│  │ .c     │ │ .c      │ │ .c     │ │ (kmod stub)    │   │
│  └────────┘ └─────────┘ └────────┘ └────────────────┘   │
├─────────────────────────────────────────────────────────┤
│  Linux Kernel                                           │
│  ┌──────────┐ ┌───────────┐ ┌────────┐ ┌────────────┐   │
│  │perf_event│ │ sysfs/    │ │ netns  │ │ benchmon   │   │
│  │  (CPU)   │ │ procfs    │ │ + veth │ │ _net kmod  │   │
│  └──────────┘ └───────────┘ └────────┘ └────────────┘   │
└─────────────────────────────────────────────────────────┘
```

## Two-Phase Design

### Phase 1: Setup (may require reboot)

The library configures the system according to the paper's methodology:

| Step                    | What it does                              | Reboot? |
|------------------------|-------------------------------------------|---------|
| GRUB / kernel params   | `isolcpus`, `nohz_full`, `rcu_nocbs`      | **Yes** |
| SMT disable            | Runtime disable or BIOS recommendation    | Maybe   |
| Frequency locking      | Disable boost, governor → performance     | No      |
| IRQ affinity           | Migrate all IRQs to housekeeping core     | No      |
| Service shutdown       | Stop irqbalance, cron, snapd, etc.        | No      |
| Swap disable           | `swapoff -a`                              | No      |
| Network namespaces     | Create ns-server/ns-client + veth pair    | No      |
| Offloading disable     | TSO/GSO/GRO off on veth interfaces        | No      |
| NetEm                  | Apply delay/jitter/loss on veth           | No      |

When kernel parameters are modified, the library returns `BENCHMON_ERR_REBOOT`
and the TUI shows a clear **"REBOOT REQUIRED"** message.

### Phase 2: Monitor (zero-alloc hot path)

Once the system is configured, the monitor samples CPU, memory, network,
and disk at ~1-5 μs per snapshot:

- **CPU**: perf_event hardware counters (instructions, cycles, cache misses),
  frequency via sysfs — all file descriptors pre-opened at init
- **Memory**: `sysinfo()` syscall (~200ns)
- **Network**: sysfs counters via `pread()` on pre-opened fds
  (or kernel passthrough module for packet-level timestamping)
- **Disk**: `/sys/block/*/stat` via `pread()`

No `malloc`, no `open`/`close`, no blocking I/O on the hot path.

## TUI Tabs

| Tab       | Key | Purpose                                        |
|-----------|-----|------------------------------------------------|
| Dashboard | F1  | System overview, verification checklist         |
| Setup     | F2  | Interactive config wizard, apply/teardown       |
| Monitor   | F3  | Live sparkline charts (CPU/MEM/NET/DISK)       |
| Logs      | F4  | Timestamped setup and runtime log              |
| Terminal  | F5  | Embedded shell — run any Linux command          |

### Terminal passthrough

The Terminal tab lets you run arbitrary Linux commands without leaving
benchmon. Type `verify` for a quick system check, or run `tc qdisc show`,
`lscpu -e`, `ip netns exec ns-server ping 10.0.0.2`, etc.

## Build

### Prerequisites

```bash
# Debian/Ubuntu
sudo apt install build-essential libclang-dev ethtool iproute2

# Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### C library only

```bash
cd benchmon
make
# -> libbenchmon.so, libbenchmon.a
```

### Full TUI (recommended)

```bash
cd benchmon/tui
cargo build --release
# -> target/release/benchmon
```

### Install

**Option 1: Pre-built Package (Recommended)**

The easiest way to install on Debian/Ubuntu is to download the `.deb` from the GitHub Releases page. This single package gives you everything automatically: the interactive `benchmon` TUI dashboard, the C libraries (`libbenchmon.so`/`.a`), and the headers (`benchmon.h`).

```bash
# Note: Check the Releases page for the specific version tag
wget https://github.com/Lordnns/benchmon/releases/download/{VERSION}/benchmon.deb
sudo dpkg -i benchmon.deb
```

**Option 2: Build from Source**

If you prefer compiling things yourself:
```bash
# 1. Install Library & Headers
sudo make install   # installs to /usr/local/lib + /usr/local/include

# 2. Install TUI binary (requires 'cargo build --release' in tui/ directory)
sudo cp tui/target/release/benchmon /usr/local/bin/
```

### Developing with libbenchmon

Once installed (using either method), you can freely link the tool inside your C/C++ projects:

```c
#include <benchmon.h>
```
Compile with:
```bash
gcc -o my_app my_app.c -lbenchmon
```

## Usage

```bash
# Run as root for full functionality
sudo benchmon

# Or without root (monitor only, no setup)
benchmon
```

## Kernel Passthrough Module (Future)

The network monitoring has a stub for Valère's kernel passthrough module
(`benchmon_net_kmod`). When loaded, it:

1. Hooks into the NIC driver's packet path
2. Stamps each packet with hardware + software timestamps
3. Writes to a lock-free ring buffer (shared memory)
4. Userspace reads via `mmap()` — **zero syscalls on the read path**

The API is already defined in `benchmon.h`:
- `benchmon_net_passthru_available()` — check if module is loaded
- `benchmon_net_passthru_open(iface)` — open ring buffer
- `benchmon_net_passthru_read(fd, events, max)` — non-blocking batch read
- `benchmon_net_passthru_close(fd)` — cleanup

## API (C)

```c
#include <benchmon.h>

// Phase 1: Setup
benchmon_setup_config_t cfg = { ... };
benchmon_setup_result_t result;
benchmon_status_t rc = benchmon_setup(&cfg, &result);
if (rc == BENCHMON_ERR_REBOOT) {
    printf("Reboot required: %s\n", result.message);
}

// Phase 2: Monitor
benchmon_monitor_t *mon = benchmon_monitor_init(BENCHMON_MON_ALL, NULL, 0);
benchmon_snapshot_t snap;
benchmon_snapshot(mon, &snap);  // ~1-5 μs
printf("Capture latency: %lu ns\n", snap.capture_latency_ns);

// Cleanup
benchmon_monitor_destroy(mon);
benchmon_teardown(&cfg);
```

## License

MIT
