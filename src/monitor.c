/*
 * monitor.c — Phase 2: Zero-alloc, constant-time system monitoring
 *
 * All file descriptors are opened once in _init() and kept open.
 * The hot path (_snapshot) uses only pread() — no open/close,
 * no malloc, no syscalls that can block.
 */

#define _GNU_SOURCE
#include "benchmon_internal.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <fcntl.h>
#include <dirent.h>
#include <time.h>
#include <sys/mman.h>
#include <linux/perf_event.h>
#include <sys/ioctl.h>
#include <sys/syscall.h>

/* Suppress warn_unused_result for perf reads on the hot path. */
#define IGNORE_RESULT(x) do { if ((x)) {} } while (0)

/* ------------------------------------------------------------------ */
/*  perf_event helpers                                                 */
/* ------------------------------------------------------------------ */

static int perf_open(int cpu, uint32_t type, uint64_t config) {
    struct perf_event_attr pe;
    memset(&pe, 0, sizeof(pe));
    pe.type           = type;
    pe.size           = sizeof(pe);
    pe.config         = config;
    pe.disabled       = 0;
    pe.exclude_kernel = 0;
    pe.exclude_hv     = 1;

    int fd = (int)syscall(__NR_perf_event_open, &pe, -1, cpu, -1, 0);
    return fd;  /* -1 on failure (no permission, etc.) */
}

/* ------------------------------------------------------------------ */
/*  Fast manual integer parser — replaces sscanf on hot path          */
/* ------------------------------------------------------------------ */

static inline uint64_t parse_next_u64(const char **p) {
    while (**p && (**p < '0' || **p > '9')) (*p)++;
    uint64_t v = 0;
    while (**p >= '0' && **p <= '9') v = v * 10 + (uint64_t)(*(*p)++ - '0');
    return v;
}

/* ------------------------------------------------------------------ */
/*  Fast meminfo parser via pre-opened fd — replaces sysinfo()        */
/* ------------------------------------------------------------------ */

static void read_meminfo(int fd, benchmon_mem_sample_t *m) {
    char buf[512];
    ssize_t n = pread(fd, buf, sizeof(buf) - 1, 0);
    if (n <= 0) return;
    buf[n] = '\0';

    uint64_t total = 0, free_ = 0, avail = 0, buffers = 0, cached = 0,
             swap_total = 0, swap_free = 0;

    char *p = buf;
    while (*p) {
        if      (strncmp(p, "MemTotal:",     9)  == 0) { const char *q = p; total      = parse_next_u64(&q); }
        else if (strncmp(p, "MemFree:",      8)  == 0) { const char *q = p; free_      = parse_next_u64(&q); }
        else if (strncmp(p, "MemAvailable:", 13) == 0) { const char *q = p; avail      = parse_next_u64(&q); }
        else if (strncmp(p, "Buffers:",      8)  == 0) { const char *q = p; buffers    = parse_next_u64(&q); }
        else if (strncmp(p, "Cached:",       7)  == 0) { const char *q = p; cached     = parse_next_u64(&q); }
        else if (strncmp(p, "SwapTotal:",   10)  == 0) { const char *q = p; swap_total = parse_next_u64(&q); }
        else if (strncmp(p, "SwapFree:",     9)  == 0) { const char *q = p; swap_free  = parse_next_u64(&q); }
        while (*p && *p != '\n') p++;
        if (*p) p++;
    }

    /* /proc/meminfo is in kB */
    m->total_bytes     = total      * 1024;
    m->available_bytes = avail      * 1024;
    m->used_bytes      = (total - free_ - buffers - cached) * 1024;
    m->cache_bytes     = (buffers + cached) * 1024;
    m->swap_used_bytes = (swap_total - swap_free) * 1024;
}

/* ------------------------------------------------------------------ */
/*  Open sysfs counters for a network interface                        */
/* ------------------------------------------------------------------ */

static int open_net_stat(const char *iface, const char *stat_name) {
    char path[512];
    snprintf(path, sizeof(path),
             "/sys/class/net/%s/statistics/%s", iface, stat_name);
    return open(path, O_RDONLY);
}

static int init_net_ctx(benchmon_net_ctx_t *ctx, const char *iface) {
    memset(ctx, 0, sizeof(*ctx));
    snprintf(ctx->iface, sizeof(ctx->iface), "%s", iface);
    ctx->passthru_fd = -1;

    ctx->fd_rx_bytes   = open_net_stat(iface, "rx_bytes");
    ctx->fd_tx_bytes   = open_net_stat(iface, "tx_bytes");
    ctx->fd_rx_packets = open_net_stat(iface, "rx_packets");
    ctx->fd_tx_packets = open_net_stat(iface, "tx_packets");
    ctx->fd_rx_dropped = open_net_stat(iface, "rx_dropped");
    ctx->fd_tx_dropped = open_net_stat(iface, "tx_dropped");
    ctx->fd_rx_errors  = open_net_stat(iface, "rx_errors");
    ctx->fd_tx_errors  = open_net_stat(iface, "tx_errors");

    return (ctx->fd_rx_bytes >= 0) ? 0 : -1;
}

/* ------------------------------------------------------------------ */
/*  Open netns veth stats via /proc/<pid>/root — no setns needed      */
/*                                                                     */
/*  Requires a process running inside the namespace.                   */
/*  setup.c starts `sleep infinity` in each ns for this purpose.      */
/* ------------------------------------------------------------------ */

static pid_t get_netns_pid(const char *netns_name) {
    char cmd[256];
    snprintf(cmd, sizeof(cmd),
             "ip netns pids %s 2>/dev/null | head -1", netns_name);

    FILE *fp = popen(cmd, "r");
    if (!fp) return -1;

    char buf[32] = {0};
    if (fgets(buf, sizeof(buf), fp) == NULL) {
        pclose(fp);
        return -1;
    }
    pclose(fp);

    buf[strcspn(buf, "\n")] = '\0';
    if (buf[0] == '\0') return -1;
    return (pid_t)atoi(buf);
}

static int open_netns_stat(pid_t pid, const char *iface,
                            const char *stat_name) {
    char path[512];
    snprintf(path, sizeof(path),
             "/proc/%d/root/sys/class/net/%s/statistics/%s",
             (int)pid, iface, stat_name);
    return open(path, O_RDONLY);
}

static int init_net_ctx_via_procfs(benchmon_net_ctx_t *ctx,
                                    const char *netns_name,
                                    const char *iface,
                                    const char *label)
{
    pid_t pid = get_netns_pid(netns_name);
    if (pid <= 0) return -1;

    memset(ctx, 0, sizeof(*ctx));
    snprintf(ctx->iface, sizeof(ctx->iface), "%s", label);
    ctx->passthru_fd = -1;

    ctx->fd_rx_bytes   = open_netns_stat(pid, iface, "rx_bytes");
    ctx->fd_tx_bytes   = open_netns_stat(pid, iface, "tx_bytes");
    ctx->fd_rx_packets = open_netns_stat(pid, iface, "rx_packets");
    ctx->fd_tx_packets = open_netns_stat(pid, iface, "tx_packets");
    ctx->fd_rx_dropped = open_netns_stat(pid, iface, "rx_dropped");
    ctx->fd_tx_dropped = open_netns_stat(pid, iface, "tx_dropped");
    ctx->fd_rx_errors  = open_netns_stat(pid, iface, "rx_errors");
    ctx->fd_tx_errors  = open_netns_stat(pid, iface, "tx_errors");

    return (ctx->fd_rx_bytes >= 0) ? 0 : -1;
}

/* ------------------------------------------------------------------ */
/*  Discover network interfaces (non-lo)                               */
/* ------------------------------------------------------------------ */

static int discover_net_interfaces(benchmon_monitor_t *mon) {
    DIR *d = opendir("/sys/class/net");
    if (!d) return 0;

    struct dirent *e;
    mon->net_count = 0;
    while ((e = readdir(d)) && mon->net_count < 8) {
        if (e->d_name[0] == '.') continue;
        if (strcmp(e->d_name, "lo") == 0) continue;
        if (init_net_ctx(&mon->nets[mon->net_count], e->d_name) == 0)
            mon->net_count++;
    }
    closedir(d);
    return mon->net_count;
}

/* ------------------------------------------------------------------ */
/*  Read ns/veth names from active_config.json (falls back to defaults) */
/* ------------------------------------------------------------------ */
 
static int read_cfg_str(const char *key, char *out, size_t out_len) {
    FILE *fp = fopen("/var/lib/benchmon/active_config.json", "r");
    if (!fp) return -1;
 
    char buf[4096];
    size_t n = fread(buf, 1, sizeof(buf) - 1, fp);
    fclose(fp);
    buf[n] = '\0';
 
    char needle[64];
    snprintf(needle, sizeof(needle), "\"%s\":", key);
    char *p = strstr(buf, needle);
    if (!p) return -1;
    p += strlen(needle);
    while (*p == ' ' || *p == '\t') p++;
    if (*p != '"') return -1;
    p++;
    char *end = strchr(p, '"');
    if (!end) return -1;
 
    size_t len = (size_t)(end - p);
    if (len >= out_len) len = out_len - 1;
    memcpy(out, p, len);
    out[len] = '\0';
    return 0;
}
 
static void discover_netns_interfaces(benchmon_monitor_t *mon) {
    char ns_s[64] = "ns_server";
    char ns_c[64] = "ns_client";
    char ve_s[64] = "veth-srv";
    char ve_c[64] = "veth-cli";
 
    /* Override from active config if present */
    read_cfg_str("ns_server",   ns_s, sizeof(ns_s));
    read_cfg_str("ns_client",   ns_c, sizeof(ns_c));
    read_cfg_str("veth_server", ve_s, sizeof(ve_s));
    read_cfg_str("veth_client", ve_c, sizeof(ve_c));
 
    /* Label uses short tag so it fits in the 16-byte iface field */
    char label_s[16], label_c[16];
    snprintf(label_s, sizeof(label_s), "s:%s", ve_s);
    snprintf(label_c, sizeof(label_c), "c:%s", ve_c);
 
    if (mon->net_count < 8) {
        if (init_net_ctx_via_procfs(&mon->nets[mon->net_count],
                                    ns_s, ve_s, label_s) == 0)
            mon->net_count++;
    }
    if (mon->net_count < 8) {
        if (init_net_ctx_via_procfs(&mon->nets[mon->net_count],
                                    ns_c, ve_c, label_c) == 0)
            mon->net_count++;
    }
}

/* ------------------------------------------------------------------ */
/*  Discover block devices                                             */
/* ------------------------------------------------------------------ */

static int discover_block_devices(benchmon_monitor_t *mon) {
    DIR *d = opendir("/sys/block");
    if (!d) return 0;

    struct dirent *e;
    mon->disk_count = 0;
    while ((e = readdir(d)) && mon->disk_count < 8) {
        if (e->d_name[0] == '.') continue;
        if (strncmp(e->d_name, "loop", 4) == 0) continue;
        if (strncmp(e->d_name, "ram",  3) == 0) continue;
        if (strncmp(e->d_name, "dm-",  3) == 0) continue;

        benchmon_disk_ctx_t *ctx = &mon->disks[mon->disk_count];
        memset(ctx, 0, sizeof(*ctx));
        snprintf(ctx->device, sizeof(ctx->device), "%s", e->d_name);

        char path[512];
        snprintf(path, sizeof(path), "/sys/block/%s/stat", e->d_name);
        ctx->fd_stat = open(path, O_RDONLY);
        if (ctx->fd_stat >= 0)
            mon->disk_count++;
    }
    closedir(d);
    return mon->disk_count;
}

/* ------------------------------------------------------------------ */
/*  Public: benchmon_monitor_init()                                    */
/* ------------------------------------------------------------------ */

benchmon_monitor_t *benchmon_monitor_init(benchmon_mon_flags_t flags,
                                          const int *cores,
                                          int core_count)
{
    benchmon_monitor_t *mon = calloc(1, sizeof(*mon));
    if (!mon) {
        BENCHMON_SET_ERR("malloc failed");
        return NULL;
    }
    mon->flags = flags;

    /* Lock the monitor struct in RAM — eliminates page fault jitter */
    mlock(mon, sizeof(*mon));

    /* Open /proc/meminfo once, keep fd for hot path */
    mon->fd_meminfo = open("/proc/meminfo", O_RDONLY);

    /* CPU cores ---------------------------------------------------- */
    if (flags & BENCHMON_MON_CPU) {
        if (cores && core_count > 0) {
            mon->core_count = core_count > 64 ? 64 : core_count;
            for (int i = 0; i < mon->core_count; i++) {
                benchmon_core_ctx_t *c = &mon->cores[i];
                c->core_id = cores[i];
                c->fd_instructions = perf_open(cores[i],
                    PERF_TYPE_HARDWARE, PERF_COUNT_HW_INSTRUCTIONS);
                c->fd_cycles = perf_open(cores[i],
                    PERF_TYPE_HARDWARE, PERF_COUNT_HW_CPU_CYCLES);
                c->fd_cache_misses = perf_open(cores[i],
                    PERF_TYPE_HARDWARE, PERF_COUNT_HW_CACHE_MISSES);

                char path[128];
                snprintf(path, sizeof(path),
                    "/sys/devices/system/cpu/cpu%d/cpufreq/scaling_cur_freq",
                    cores[i]);
                c->fd_freq = open(path, O_RDONLY);
            }
        } else {
            int n = (int)sysconf(_SC_NPROCESSORS_ONLN);
            if (n > 64) n = 64;
            mon->core_count = n;
            for (int i = 0; i < n; i++) {
                benchmon_core_ctx_t *c = &mon->cores[i];
                c->core_id = i;
                c->fd_instructions = perf_open(i,
                    PERF_TYPE_HARDWARE, PERF_COUNT_HW_INSTRUCTIONS);
                c->fd_cycles = perf_open(i,
                    PERF_TYPE_HARDWARE, PERF_COUNT_HW_CPU_CYCLES);
                c->fd_cache_misses = perf_open(i,
                    PERF_TYPE_HARDWARE, PERF_COUNT_HW_CACHE_MISSES);

                char path[128];
                snprintf(path, sizeof(path),
                    "/sys/devices/system/cpu/cpu%d/cpufreq/scaling_cur_freq",
                    i);
                c->fd_freq = open(path, O_RDONLY);
            }
        }
    }

    /* Network ------------------------------------------------------ */
    if (flags & BENCHMON_MON_NETWORK) {
        discover_net_interfaces(mon);
        discover_netns_interfaces(mon);
    }

    /* Disk --------------------------------------------------------- */
    if (flags & BENCHMON_MON_DISK)
        discover_block_devices(mon);

    return mon;
}

/* ------------------------------------------------------------------ */
/*  HOT PATH: benchmon_snapshot()                                      */
/*                                                                     */
/*  No malloc.  No open/close.  Only pread + read on pre-opened fds.   */
/*  Single timestamp shared across all subsystems.                     */
/* ------------------------------------------------------------------ */

benchmon_status_t benchmon_snapshot(benchmon_monitor_t *mon,
                                    benchmon_snapshot_t *snap)
{
    /* ONE timestamp for the entire snapshot — no extra clock calls */
    uint64_t t0 = benchmon_now_ns();

    memset(snap, 0, sizeof(*snap));
    snap->timestamp_ns = t0;

    /* ---- CPU ---- */
    if (mon->flags & BENCHMON_MON_CPU) {
        snap->cpu_count = mon->core_count;
        for (int i = 0; i < mon->core_count; i++) {
            benchmon_core_ctx_t   *c = &mon->cores[i];
            benchmon_cpu_sample_t *s = &snap->cpu[i];

            s->core_id      = c->core_id;
            s->timestamp_ns = t0;
            s->tsc          = benchmon_rdtsc();

            if (c->fd_instructions >= 0)
                IGNORE_RESULT(read(c->fd_instructions, &s->instructions, 8));
            if (c->fd_cycles >= 0)
                IGNORE_RESULT(read(c->fd_cycles, &s->cycles, 8));
            if (c->fd_cache_misses >= 0)
                IGNORE_RESULT(read(c->fd_cache_misses, &s->cache_misses, 8));
            if (c->fd_freq >= 0)
                s->freq_mhz = (uint32_t)(benchmon_pread_uint64(c->fd_freq)
                               / 1000);  /* kHz → MHz */
        }
    }

    /* ---- Memory: pread /proc/meminfo — no sysinfo() syscall ---- */
    if (mon->flags & BENCHMON_MON_MEMORY) {
        snap->mem.timestamp_ns = t0;
        if (mon->fd_meminfo >= 0)
            read_meminfo(mon->fd_meminfo, &snap->mem);
    }

    /* ---- Network ---- */
    if (mon->flags & BENCHMON_MON_NETWORK) {
        snap->net_count = mon->net_count;
        for (int i = 0; i < mon->net_count; i++) {
            benchmon_net_ctx_t    *c = &mon->nets[i];
            benchmon_net_sample_t *s = &snap->net[i];

            s->timestamp_ns = t0;
            snprintf(s->iface, sizeof(s->iface), "%s", c->iface);

            s->rx_bytes   = benchmon_pread_uint64(c->fd_rx_bytes);
            s->tx_bytes   = benchmon_pread_uint64(c->fd_tx_bytes);
            s->rx_packets = benchmon_pread_uint64(c->fd_rx_packets);
            s->tx_packets = benchmon_pread_uint64(c->fd_tx_packets);
            s->rx_dropped = benchmon_pread_uint64(c->fd_rx_dropped);
            s->tx_dropped = benchmon_pread_uint64(c->fd_tx_dropped);
            s->rx_errors  = benchmon_pread_uint64(c->fd_rx_errors);
            s->tx_errors  = benchmon_pread_uint64(c->fd_tx_errors);
            s->passthru_active = (c->passthru_fd >= 0) ? 1 : 0;
        }
    }

    /* ---- Disk: manual parser replaces sscanf ---- */
    if (mon->flags & BENCHMON_MON_DISK) {
        snap->disk_count = mon->disk_count;
        for (int i = 0; i < mon->disk_count; i++) {
            benchmon_disk_ctx_t    *c = &mon->disks[i];
            benchmon_disk_sample_t *s = &snap->disk[i];

            s->timestamp_ns = t0;
            snprintf(s->device, sizeof(s->device), "%s", c->device);

            char buf[256];
            ssize_t n = pread(c->fd_stat, buf, sizeof(buf) - 1, 0);
            if (n > 0) {
                buf[n] = '\0';
                const char *p = buf;
                s->reads_completed  = parse_next_u64(&p);
                parse_next_u64(&p); /* reads_merged  — skip */
                s->read_bytes       = parse_next_u64(&p) * 512;
                parse_next_u64(&p); /* read_time_ms  — skip */
                s->writes_completed = parse_next_u64(&p);
                parse_next_u64(&p); /* writes_merged — skip */
                s->write_bytes      = parse_next_u64(&p) * 512;
                parse_next_u64(&p); /* write_time_ms — skip */
                s->io_in_progress   = parse_next_u64(&p);
                s->io_time_ms       = parse_next_u64(&p);
            }
        }
    }

    snap->capture_latency_ns = benchmon_now_ns() - t0;
    return BENCHMON_OK;
}

/* ------------------------------------------------------------------ */
/*  Public: benchmon_monitor_destroy()                                 */
/* ------------------------------------------------------------------ */

void benchmon_monitor_destroy(benchmon_monitor_t *mon) {
    if (!mon) return;

    if (mon->fd_meminfo >= 0) close(mon->fd_meminfo);

    for (int i = 0; i < mon->core_count; i++) {
        benchmon_core_ctx_t *c = &mon->cores[i];
        if (c->fd_instructions >= 0) close(c->fd_instructions);
        if (c->fd_cycles       >= 0) close(c->fd_cycles);
        if (c->fd_cache_misses >= 0) close(c->fd_cache_misses);
        if (c->fd_freq         >= 0) close(c->fd_freq);
    }
    for (int i = 0; i < mon->net_count; i++) {
        benchmon_net_ctx_t *c = &mon->nets[i];
        if (c->fd_rx_bytes   >= 0) close(c->fd_rx_bytes);
        if (c->fd_tx_bytes   >= 0) close(c->fd_tx_bytes);
        if (c->fd_rx_packets >= 0) close(c->fd_rx_packets);
        if (c->fd_tx_packets >= 0) close(c->fd_tx_packets);
        if (c->fd_rx_dropped >= 0) close(c->fd_rx_dropped);
        if (c->fd_tx_dropped >= 0) close(c->fd_tx_dropped);
        if (c->fd_rx_errors  >= 0) close(c->fd_rx_errors);
        if (c->fd_tx_errors  >= 0) close(c->fd_tx_errors);
        if (c->passthru_fd   >= 0) close(c->passthru_fd);
    }
    for (int i = 0; i < mon->disk_count; i++) {
        if (mon->disks[i].fd_stat >= 0) close(mon->disks[i].fd_stat);
    }

    munlock(mon, sizeof(*mon));
    free(mon);
}