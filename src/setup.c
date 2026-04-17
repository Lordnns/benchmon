/*
 * setup.c — Phase 1: System configuration for benchmarking
 *
 * Each sub-step is idempotent — calling setup twice is safe.
 * Returns BENCHMON_ERR_REBOOT when kernel boot params were changed.
 */

#define _GNU_SOURCE
#include "benchmon_internal.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdarg.h>
#include <unistd.h>
#include <errno.h>
#include <fcntl.h>
#include <sys/stat.h>

/* ------------------------------------------------------------------ */
/*  Helpers                                                            */
/* ------------------------------------------------------------------ */

static int is_root(void) { return geteuid() == 0; }

static void result_append(benchmon_setup_result_t *r, const char *fmt, ...) {
    size_t len = strlen(r->message);
    if (len >= sizeof(r->message) - 2) return;
    va_list ap;
    va_start(ap, fmt);
    int n = vsnprintf(r->message + len, sizeof(r->message) - len, fmt, ap);
    va_end(ap);
    if (n > 0) {
        len += (size_t)n;
        if (len < sizeof(r->message) - 2) {
            r->message[len] = '\n';
            r->message[len + 1] = '\0';
        }
    }
}

/* ------------------------------------------------------------------ */
/*  GRUB / kernel boot parameters                                      */
/* ------------------------------------------------------------------ */

static int setup_grub(const benchmon_setup_config_t *cfg,
                      benchmon_setup_result_t *res)
{
    if (!cfg->isolated_cores || cfg->isolated_cores_count == 0) return 0;

    char cores[256] = {0};
    for (int i = 0; i < cfg->isolated_cores_count; i++) {
        char t[16];
        snprintf(t, sizeof(t), "%s%d", i ? "," : "", cfg->isolated_cores[i]);
        strncat(cores, t, sizeof(cores) - strlen(cores) - 1);
    }

    char params[512];
    snprintf(params, sizeof(params),
             "isolcpus=%s nohz_full=%s rcu_nocbs=%s "
             "processor.max_cstate=%d nosoftlockup",
             cores, cores, cores,
             cfg->max_cstate >= 0 ? cfg->max_cstate : 0);

    char cmdline[2048] = {0};
    benchmon_read_sysfs_str("/proc/cmdline", cmdline, sizeof(cmdline));
    if (strstr(cmdline, "isolcpus=") && strstr(cmdline, cores)) {
        result_append(res, "GRUB: params already active (isolcpus=%s)", cores);
        return 0;
    }

    const char *gp = "/etc/default/grub";
    struct stat st;
    if (stat(gp, &st) != 0) {
        result_append(res, "GRUB: %s not found", gp);
        return -1;
    }

    benchmon_exec("cp /etc/default/grub /etc/default/grub.benchmon.bak",
                  NULL, 0);

    char sed[2048];
    snprintf(sed, sizeof(sed),
             "grep -q '^GRUB_CMDLINE_LINUX=' %s && "
             "sed -i 's|^GRUB_CMDLINE_LINUX=\"\\(.*\\)\"|"
             "GRUB_CMDLINE_LINUX=\"\\1 %s\"|' %s || "
             "echo 'GRUB_CMDLINE_LINUX=\"%s\"' >> %s",
             gp, params, gp, params, gp);
    benchmon_exec(sed, NULL, 0);

    if (benchmon_exec("update-grub 2>/dev/null || "
                      "grub2-mkconfig -o /boot/grub2/grub.cfg 2>/dev/null || "
                      "grub-mkconfig -o /boot/grub/grub.cfg 2>/dev/null",
                      NULL, 0) != 0)
        result_append(res, "GRUB: modified but could not regenerate — "
                     "run update-grub manually");

    res->grub_modified = 1;
    res->reboot_required = 1;
    result_append(res, "GRUB: added [%s] — REBOOT REQUIRED", params);
    return 1;
}

/* ------------------------------------------------------------------ */
/*  SMT                                                                */
/* ------------------------------------------------------------------ */

static int setup_smt(const benchmon_setup_config_t *cfg,
                     benchmon_setup_result_t *res)
{
    if (!cfg->disable_smt) return 0;

    char buf[16] = {0};
    benchmon_read_sysfs_str("/sys/devices/system/cpu/smt/active",
                            buf, sizeof(buf));
    if (buf[0] == '0') { res->smt_disabled = 1; return 0; }

    if (benchmon_write_sysfs_str(
            "/sys/devices/system/cpu/smt/control", "off") == 0) {
        res->smt_disabled = 1;
        result_append(res, "SMT: disabled at runtime");
        return 0;
    }
    result_append(res, "SMT: runtime disable failed — disable in BIOS");
    return -1;
}

/* ------------------------------------------------------------------ */
/*  Frequency                                                          */
/* ------------------------------------------------------------------ */

static int setup_freq(const benchmon_setup_config_t *cfg,
                      benchmon_setup_result_t *res)
{
    if (cfg->disable_frequency_boost) {
        benchmon_write_sysfs_str(
            "/sys/devices/system/cpu/intel_pstate/no_turbo", "1");
        benchmon_write_sysfs_str(
            "/sys/devices/system/cpu/cpufreq/boost", "0");
        result_append(res, "FREQ: boost disabled");
    }

    char path[128];
    for (int i = 0; i < 256; i++) {
        snprintf(path, sizeof(path),
                 "/sys/devices/system/cpu/cpu%d/cpufreq/scaling_governor", i);
        if (benchmon_write_sysfs_str(path, "performance") != 0) break;
    }
    res->frequency_locked = 1;
    result_append(res, "FREQ: governor → performance");
    return 0;
}

/* ------------------------------------------------------------------ */
/*  IRQ affinity                                                       */
/* ------------------------------------------------------------------ */

static int setup_irq(const benchmon_setup_config_t *cfg,
                     benchmon_setup_result_t *res)
{
    if (!cfg->isolated_cores || cfg->isolated_cores_count == 0) return 0;

    char mask[32];
    snprintf(mask, sizeof(mask), "%x", 1 << cfg->housekeeping_core);

    benchmon_write_sysfs_str("/proc/irq/default_smp_affinity", mask);

    char cmd[256];
    snprintf(cmd, sizeof(cmd),
             "for f in /proc/irq/[0-9]*/smp_affinity; do "
             "echo '%s' > \"$f\" 2>/dev/null; done", mask);
    benchmon_exec(cmd, NULL, 0);

    res->irq_migrated = 1;
    result_append(res, "IRQ: migrated to core %d", cfg->housekeeping_core);
    return 0;
}

/* ------------------------------------------------------------------ */
/*  Services / swap                                                    */
/* ------------------------------------------------------------------ */

static void setup_services(const benchmon_setup_config_t *cfg,
                           benchmon_setup_result_t *res)
{
    if (cfg->stop_irqbalance)
        benchmon_exec("systemctl stop irqbalance 2>/dev/null", NULL, 0);
    if (cfg->isolate_multiuser)
        benchmon_exec("systemctl isolate multi-user.target 2>/dev/null",
                     NULL, 0);

    const char *noisy[] = {"cron","anacron","atd","snapd",
                           "unattended-upgrades","packagekitd",NULL};
    for (int i = 0; noisy[i]; i++) {
        char c[128];
        snprintf(c, sizeof(c), "systemctl stop %s 2>/dev/null", noisy[i]);
        benchmon_exec(c, NULL, 0);
    }
    res->services_stopped = 1;
    result_append(res, "SERVICES: noisy daemons stopped");
}

static void setup_swap(const benchmon_setup_config_t *cfg,
                       benchmon_setup_result_t *res)
{
    if (!cfg->disable_swap) return;
    benchmon_exec("swapoff -a", NULL, 0);
    res->swap_disabled = 1;
    result_append(res, "SWAP: disabled");
}

/* ------------------------------------------------------------------ */
/*  Sysctl preconfig — capture current values before modifying        */
/* ------------------------------------------------------------------ */

#define PRECONFIG_SYSCTL "/var/lib/benchmon/preconfig_sysctl.json"

static void capture_sysctl_preconfig(void) {
    struct stat st;
    if (stat(PRECONFIG_SYSCTL, &st) == 0) return; /* already captured */

    long aslr = 2, rmem_max = 212992, wmem_max = 212992;
    long rmem_def = 212992, wmem_def = 212992, backlog = 1000;

    benchmon_read_sysfs_int("/proc/sys/kernel/randomize_va_space", &aslr);
    benchmon_read_sysfs_int("/proc/sys/net/core/rmem_max",          &rmem_max);
    benchmon_read_sysfs_int("/proc/sys/net/core/wmem_max",          &wmem_max);
    benchmon_read_sysfs_int("/proc/sys/net/core/rmem_default",      &rmem_def);
    benchmon_read_sysfs_int("/proc/sys/net/core/wmem_default",      &wmem_def);
    benchmon_read_sysfs_int("/proc/sys/net/core/netdev_max_backlog", &backlog);

    char tstate[32] = {0};
    benchmon_exec("systemctl is-active systemd-timesyncd 2>/dev/null",
                  tstate, sizeof(tstate));
    int timesyncd_active = (strncmp(tstate, "active", 6) == 0) ? 1 : 0;

    benchmon_exec("mkdir -p /var/lib/benchmon", NULL, 0);

    char json[512];
    snprintf(json, sizeof(json),
             "{\n"
             "  \"aslr\": %ld,\n"
             "  \"rmem_max\": %ld,\n"
             "  \"wmem_max\": %ld,\n"
             "  \"rmem_default\": %ld,\n"
             "  \"wmem_default\": %ld,\n"
             "  \"netdev_max_backlog\": %ld,\n"
             "  \"timesyncd_was_active\": %s\n"
             "}\n",
             aslr, rmem_max, wmem_max,
             rmem_def, wmem_def, backlog,
             timesyncd_active ? "true" : "false");

    /* Use fopen/fputs to avoid warn_unused_result on write() */
    FILE *fp = fopen(PRECONFIG_SYSCTL, "w");
    if (fp) {
        fputs(json, fp);
        fclose(fp);
    }
}

static void restore_sysctl_preconfig(void) {
    FILE *fp = fopen(PRECONFIG_SYSCTL, "r");
    if (!fp) return;

    char buf[512] = {0};
    size_t n = fread(buf, 1, sizeof(buf) - 1, fp);
    fclose(fp);
    buf[n] = '\0';

    #define EXTRACT_INT(key, dfl) ({ \
        long _v = (dfl); \
        char *_p = strstr(buf, "\"" key "\":"); \
        if (_p) { _p += strlen("\"" key "\":"); while (*_p == ' ') _p++; _v = strtol(_p, NULL, 10); } \
        _v; \
    })

    long aslr     = EXTRACT_INT("aslr",                   2);
    long rmem_max = EXTRACT_INT("rmem_max",           212992);
    long wmem_max = EXTRACT_INT("wmem_max",           212992);
    long rmem_def = EXTRACT_INT("rmem_default",       212992);
    long wmem_def = EXTRACT_INT("wmem_default",       212992);
    long backlog  = EXTRACT_INT("netdev_max_backlog",   1000);

    #undef EXTRACT_INT

    char *tp = strstr(buf, "\"timesyncd_was_active\":");
    int timesyncd_was_active = 0;
    if (tp) {
        tp += strlen("\"timesyncd_was_active\":");
        while (*tp == ' ') tp++;
        timesyncd_was_active = (strncmp(tp, "true", 4) == 0);
    }

    char cmd[256];
    snprintf(cmd, sizeof(cmd),
             "sysctl -w kernel.randomize_va_space=%ld 2>/dev/null", aslr);
    benchmon_exec(cmd, NULL, 0);
    snprintf(cmd, sizeof(cmd),
             "sysctl -w net.core.rmem_max=%ld 2>/dev/null", rmem_max);
    benchmon_exec(cmd, NULL, 0);
    snprintf(cmd, sizeof(cmd),
             "sysctl -w net.core.wmem_max=%ld 2>/dev/null", wmem_max);
    benchmon_exec(cmd, NULL, 0);
    snprintf(cmd, sizeof(cmd),
             "sysctl -w net.core.rmem_default=%ld 2>/dev/null", rmem_def);
    benchmon_exec(cmd, NULL, 0);
    snprintf(cmd, sizeof(cmd),
             "sysctl -w net.core.wmem_default=%ld 2>/dev/null", wmem_def);
    benchmon_exec(cmd, NULL, 0);
    snprintf(cmd, sizeof(cmd),
             "sysctl -w net.core.netdev_max_backlog=%ld 2>/dev/null", backlog);
    benchmon_exec(cmd, NULL, 0);

    if (timesyncd_was_active)
        benchmon_exec("systemctl start systemd-timesyncd 2>/dev/null", NULL, 0);

    remove(PRECONFIG_SYSCTL);
}

/* ------------------------------------------------------------------ */
/*  Sysctl tuning                                                      */
/* ------------------------------------------------------------------ */

static void setup_sysctl(const benchmon_setup_config_t *cfg,
                         benchmon_setup_result_t *res)
{
    int did = 0;

    if (cfg->disable_aslr) {
        benchmon_exec("sysctl -w kernel.randomize_va_space=0 2>/dev/null",
                      NULL, 0);
        result_append(res, "SYSCTL: ASLR disabled");
        did = 1;
    }
    if (cfg->tune_net_buffers) {
        benchmon_exec("sysctl -w net.core.rmem_max=26214400 2>/dev/null",
                      NULL, 0);
        benchmon_exec("sysctl -w net.core.wmem_max=26214400 2>/dev/null",
                      NULL, 0);
        benchmon_exec("sysctl -w net.core.rmem_default=1048576 2>/dev/null",
                      NULL, 0);
        benchmon_exec("sysctl -w net.core.wmem_default=1048576 2>/dev/null",
                      NULL, 0);
        benchmon_exec("sysctl -w net.core.netdev_max_backlog=5000 2>/dev/null",
                      NULL, 0);
        result_append(res, "SYSCTL: net buffers tuned");
        did = 1;
    }
    if (cfg->stop_timesyncd) {
        benchmon_exec("systemctl stop systemd-timesyncd 2>/dev/null", NULL, 0);
        benchmon_exec("systemctl stop NetworkManager-wait-online 2>/dev/null",
                      NULL, 0);
        result_append(res, "SYSCTL: timesyncd stopped");
        did = 1;
    }
    if (cfg->drop_caches) {
        benchmon_exec("sync && echo 3 > /proc/sys/vm/drop_caches 2>/dev/null",
                      NULL, 0);
        res->caches_dropped = 1;
        result_append(res, "CACHE: page cache dropped");
        did = 1;
    }
    if (did) res->sysctl_tuned = 1;
}

/* ------------------------------------------------------------------ */
/*  Process isolation config                                           */
/* ------------------------------------------------------------------ */

static void setup_process_isolation(const benchmon_setup_config_t *cfg,
                                    benchmon_setup_result_t *res)
{
    if ((!cfg->server_cores || cfg->server_cores[0] == '\0') &&
        cfg->rt_priority == 0) return;

    if (cfg->server_cores && cfg->server_cores[0] != '\0') {
        char cmd[256];
        snprintf(cmd, sizeof(cmd), "taskset -c %s true 2>/dev/null",
                 cfg->server_cores);
        if (benchmon_exec(cmd, NULL, 0) != 0) {
            result_append(res,
                "PROC: WARNING — server_cores '%s' invalid",
                cfg->server_cores);
            return;
        }
    }
    if (cfg->client_cores && cfg->client_cores[0] != '\0') {
        char cmd[256];
        snprintf(cmd, sizeof(cmd), "taskset -c %s true 2>/dev/null",
                 cfg->client_cores);
        if (benchmon_exec(cmd, NULL, 0) != 0) {
            result_append(res,
                "PROC: WARNING — client_cores '%s' invalid",
                cfg->client_cores);
            return;
        }
    }

    res->process_isolation_ready = 1;
    result_append(res, "PROC: server=%s client=%s rt=%d",
        cfg->server_cores ? cfg->server_cores : "(unset)",
        cfg->client_cores ? cfg->client_cores : "(unset)",
        cfg->rt_priority);
}

/* ------------------------------------------------------------------ */
/*  Network namespaces + veth + netem                                  */
/* ------------------------------------------------------------------ */

static int setup_network(const benchmon_setup_config_t *cfg,
                         benchmon_setup_result_t *res)
{
    if (!cfg->ns_server_name || !cfg->ns_client_name) return 0;

    const char *ns_s = cfg->ns_server_name;
    const char *ns_c = cfg->ns_client_name;
    const char *ve_s = cfg->veth_server_name ? cfg->veth_server_name : "veth-s";
    const char *ve_c = cfg->veth_client_name ? cfg->veth_client_name : "veth-c";
    const char *ip_s = cfg->server_ip        ? cfg->server_ip        : "10.0.0.1/24";
    const char *ip_c = cfg->client_ip        ? cfg->client_ip        : "10.0.0.2/24";

    char cmd[1024];

    /* Kill any lingering sleep processes from previous setup */
    snprintf(cmd, sizeof(cmd),
             "pkill -f 'ip netns exec %s sleep' 2>/dev/null; "
             "pkill -f 'ip netns exec %s sleep' 2>/dev/null",
             ns_s, ns_c);
    benchmon_exec(cmd, NULL, 0);

    /* Idempotent cleanup */
    snprintf(cmd, sizeof(cmd),
             "ip netns del %s 2>/dev/null; ip netns del %s 2>/dev/null; "
             "ip link del %s 2>/dev/null", ns_s, ns_c, ve_s);
    benchmon_exec(cmd, NULL, 0);

    /* Create namespaces + veth */
    snprintf(cmd, sizeof(cmd),
             "ip netns add %s && ip netns add %s && "
             "ip link add %s type veth peer name %s && "
             "ip link set %s netns %s && ip link set %s netns %s",
             ns_s, ns_c, ve_s, ve_c, ve_s, ns_s, ve_c, ns_c);
    if (benchmon_exec(cmd, NULL, 0) != 0) {
        result_append(res, "NET: namespace/veth creation failed");
        return -1;
    }

    /* IP + up (server) */
    snprintf(cmd, sizeof(cmd),
             "ip netns exec %s sh -c '"
             "ip addr add %s dev %s && ip link set %s up && ip link set lo up'",
             ns_s, ip_s, ve_s, ve_s);
    benchmon_exec(cmd, NULL, 0);

    /* IP + up (client) */
    snprintf(cmd, sizeof(cmd),
             "ip netns exec %s sh -c '"
             "ip addr add %s dev %s && ip link set %s up && ip link set lo up'",
             ns_c, ip_c, ve_c, ve_c);
    benchmon_exec(cmd, NULL, 0);

    snprintf(cmd, sizeof(cmd),
            "ip netns exec %s ip link set %s mtu 1500 && "
            "ip netns exec %s ip link set %s mtu 1500",
            ns_s, ve_s, ns_c, ve_c);
    benchmon_exec(cmd, NULL, 0);

    res->namespaces_created = 1;
    result_append(res, "NET: %s/%s ← veth %s↔%s", ns_s, ns_c, ve_s, ve_c);

    /* Disable offloading */
    if (cfg->disable_offloading) {
        snprintf(cmd, sizeof(cmd),
                 "ip netns exec %s ethtool -K %s tx off rx off tso off gso off gro off 2>/dev/null && "
                 "ip netns exec %s ethtool -K %s tx off rx off tso off gso off gro off 2>/dev/null",
                 ns_s, ve_s, ns_c, ve_c);
        if (benchmon_exec(cmd, NULL, 0) == 0) {
            res->offloading_disabled = 1;
            result_append(res, "NET: tx/rx/TSO/GSO/GRO off");
        }
    }

    /* NetEm */
    if (cfg->netem_delay_ms > 0 || cfg->netem_loss_pct > 0) {
        char args[256] = {0};
        if (cfg->netem_delay_ms > 0) {
            char t[64];
            snprintf(t, sizeof(t), "delay %dms", cfg->netem_delay_ms);
            strncat(args, t, sizeof(args) - strlen(args) - 1);
            if (cfg->netem_jitter_ms > 0) {
                snprintf(t, sizeof(t), " %dms distribution normal",
                         cfg->netem_jitter_ms);
                strncat(args, t, sizeof(args) - strlen(args) - 1);
            }
        }
        if (cfg->netem_loss_pct > 0) {
            char t[64];
            snprintf(t, sizeof(t), " loss %.2f%%", cfg->netem_loss_pct);
            strncat(args, t, sizeof(args) - strlen(args) - 1);
        }

        snprintf(cmd, sizeof(cmd),
                 "ip netns exec %s tc qdisc add dev %s root netem %s && "
                 "ip netns exec %s tc qdisc add dev %s root netem %s",
                 ns_s, ve_s, args, ns_c, ve_c, args);
        if (benchmon_exec(cmd, NULL, 0) == 0) {
            res->netem_applied = 1;
            result_append(res, "NET: netem [%s]", args);
        }
    }

    /*
     * Start a long-lived process inside each namespace so that
     * monitor.c can open veth stats via /proc/<pid>/root/sys/...
     * without needing setns() (which fails in multithreaded processes).
     */
    snprintf(cmd, sizeof(cmd),
             "ip netns exec %s sleep infinity </dev/null >/dev/null 2>&1 &",
             ns_s);
    benchmon_exec(cmd, NULL, 0);

    snprintf(cmd, sizeof(cmd),
             "ip netns exec %s sleep infinity </dev/null >/dev/null 2>&1 &",
             ns_c);
    benchmon_exec(cmd, NULL, 0);

    result_append(res, "NET: monitor anchors started in %s and %s",
                  ns_s, ns_c);

    return 0;
}

/* ------------------------------------------------------------------ */
/*  Public: benchmon_setup()                                           */
/* ------------------------------------------------------------------ */

benchmon_status_t benchmon_setup(const benchmon_setup_config_t *cfg,
                                 benchmon_setup_result_t *res)
{
    memset(res, 0, sizeof(*res));

    if (!is_root()) {
        res->status = BENCHMON_ERR_PERM;
        snprintf(res->message, sizeof(res->message),
                 "benchmon_setup requires root");
        return BENCHMON_ERR_PERM;
    }

    int err = 0;
    capture_sysctl_preconfig();
    if (setup_grub(cfg, res)    < 0) err++;
    if (setup_smt(cfg, res)     < 0) err++;
    setup_freq(cfg, res);
    if (setup_irq(cfg, res)     < 0) err++;
    setup_services(cfg, res);
    setup_swap(cfg, res);
    setup_sysctl(cfg, res);
    setup_process_isolation(cfg, res);
    if (setup_network(cfg, res) < 0) err++;

    res->status = res->reboot_required ? BENCHMON_ERR_REBOOT
                : err                  ? BENCHMON_ERR_PARTIAL
                :                        BENCHMON_OK;
    return res->status;
}

/* ------------------------------------------------------------------ */
/*  Public: benchmon_teardown()                                        */
/* ------------------------------------------------------------------ */

benchmon_status_t benchmon_teardown(const benchmon_setup_config_t *cfg) {
    if (!is_root()) return BENCHMON_ERR_PERM;

    /* Kill monitor anchor processes */
    if (cfg->ns_server_name) {
        char c[256];
        snprintf(c, sizeof(c),
                 "pkill -f 'ip netns exec %s sleep' 2>/dev/null",
                 cfg->ns_server_name);
        benchmon_exec(c, NULL, 0);

        snprintf(c, sizeof(c), "ip netns del %s 2>/dev/null",
                 cfg->ns_server_name);
        benchmon_exec(c, NULL, 0);
    }
    if (cfg->ns_client_name) {
        char c[256];
        snprintf(c, sizeof(c),
                 "pkill -f 'ip netns exec %s sleep' 2>/dev/null",
                 cfg->ns_client_name);
        benchmon_exec(c, NULL, 0);

        snprintf(c, sizeof(c), "ip netns del %s 2>/dev/null",
                 cfg->ns_client_name);
        benchmon_exec(c, NULL, 0);
    }
    if (cfg->disable_swap)
        benchmon_exec("swapon -a 2>/dev/null", NULL, 0);

    restore_sysctl_preconfig();

    return BENCHMON_OK;
}

/* ------------------------------------------------------------------ */
/*  Public: benchmon_get_launch_prefix()                               */
/* ------------------------------------------------------------------ */

char *benchmon_get_launch_prefix(const benchmon_setup_config_t *cfg,
                                 int is_server)
{
    char buf[256] = {0};
    const char *cores = is_server ? cfg->server_cores : cfg->client_cores;

    if (cores && cores[0] != '\0' && cfg->rt_priority > 0)
        snprintf(buf, sizeof(buf), "taskset -c %s chrt -f %d ",
                 cores, cfg->rt_priority);
    else if (cores && cores[0] != '\0')
        snprintf(buf, sizeof(buf), "taskset -c %s ", cores);
    else if (cfg->rt_priority > 0)
        snprintf(buf, sizeof(buf), "chrt -f %d ", cfg->rt_priority);

    char *out = (char *)malloc(strlen(buf) + 1);
    if (out) strcpy(out, buf);
    return out;
}