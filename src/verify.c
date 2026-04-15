/*
 * verify.c — Pre-flight verification and system report
 *
 * Read-only: never modifies the system.
 * Does not require root (but some checks need it for full results).
 */

#define _GNU_SOURCE
#include "benchmon_internal.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/sysinfo.h>
#include <sys/utsname.h>
#include <unistd.h>

/* ------------------------------------------------------------------ */
/*  Helpers: extract names from config or active_config.json           */
/* ------------------------------------------------------------------ */

/* Defaults — match ffi.rs SetupConfig::default() */
#define DEFAULT_NS_SERVER "ns_server"
#define DEFAULT_NS_CLIENT "ns_client"
#define DEFAULT_VETH_SRV "veth-s"
#define DEFAULT_VETH_CLI "veth-c"

/*
 * Try to extract a JSON string value from the active config file.
 * Returns 0 on success, -1 if not found.
 * Minimal parser — no dependency on a JSON library.
 */
static int read_active_cfg_str(const char *key, char *out, size_t out_len) {
  const char *path = "/var/lib/benchmon/active_config.json";
  FILE *fp = fopen(path, "r");
  if (!fp)
    return -1;

  char buf[4096];
  size_t n = fread(buf, 1, sizeof(buf) - 1, fp);
  fclose(fp);
  buf[n] = '\0';

  /* Find "key": "value" */
  char needle[128];
  snprintf(needle, sizeof(needle), "\"%s\":", key);
  char *p = strstr(buf, needle);
  if (!p)
    return -1;
  p += strlen(needle);
  while (*p == ' ' || *p == '\t')
    p++;
  if (*p != '"')
    return -1;
  p++;
  char *end = strchr(p, '"');
  if (!end)
    return -1;

  size_t len = (size_t)(end - p);
  if (len >= out_len)
    len = out_len - 1;
  memcpy(out, p, len);
  out[len] = '\0';
  return 0;
}

/*
 * Resolve a name: cfg field > active_config.json > hardcoded default.
 */
static const char *resolve_name(const char *cfg_field, const char *json_key,
                                const char *fallback, char *scratch,
                                size_t scratch_len) {
  /* 1. Explicit config */
  if (cfg_field && cfg_field[0])
    return cfg_field;

  /* 2. Active config file */
  if (read_active_cfg_str(json_key, scratch, scratch_len) == 0 && scratch[0])
    return scratch;

  /* 3. Default */
  return fallback;
}

/* ------------------------------------------------------------------ */
/*  Public: benchmon_verify()                                          */
/* ------------------------------------------------------------------ */

benchmon_status_t benchmon_verify(benchmon_verify_result_t *r,
                                  const benchmon_setup_config_t *cfg) {
  memset(r, 0, sizeof(*r));
  r->all_checks_passed = 1; /* Assume good, clear on failure */

  /* Resolve dynamic names */
  char _ns_s[64], _ns_c[64], _ve_s[64], _ve_c[64];

  const char *ns_s = resolve_name(cfg ? cfg->ns_server_name : NULL, "ns_server",
                                  DEFAULT_NS_SERVER, _ns_s, sizeof(_ns_s));

  const char *ns_c = resolve_name(cfg ? cfg->ns_client_name : NULL, "ns_client",
                                  DEFAULT_NS_CLIENT, _ns_c, sizeof(_ns_c));

  const char *ve_s =
      resolve_name(cfg ? cfg->veth_server_name : NULL, "veth_server",
                   DEFAULT_VETH_SRV, _ve_s, sizeof(_ve_s));

  /* ve_c not used in verify currently, but resolve for completeness */
  (void)resolve_name(cfg ? cfg->veth_client_name : NULL, "veth_client",
                     DEFAULT_VETH_CLI, _ve_c, sizeof(_ve_c));

  char buf[512];

  /* ---- Kernel version ---- */
  struct utsname un;
  if (uname(&un) == 0)
    snprintf(r->kernel_version, sizeof(r->kernel_version), "%s", un.release);

  /* ---- CPU model ---- */
  if (benchmon_read_sysfs_str("/proc/cpuinfo", buf, sizeof(buf)) == 0) {
    char *p = strstr(buf, "model name");
    if (p) {
      p = strchr(p, ':');
      if (p) {
        p += 2;
        char *nl = strchr(p, '\n');
        if (nl)
          *nl = '\0';
        snprintf(r->cpu_model, sizeof(r->cpu_model), "%s", p);
      }
    }
  }

  /* ---- Hypervisor detection ---- */
  char hyp[128] = {0};
  if (benchmon_exec("systemd-detect-virt 2>/dev/null", hyp, sizeof(hyp)) == 0 &&
      strlen(hyp) > 0 && strcmp(hyp, "none") != 0) {
    snprintf(r->hypervisor, sizeof(r->hypervisor), "%s", hyp);
    r->running_bare_metal = 0;
    strncat(r->warnings,
            "WARNING: Running in VM/container — latency results unreliable\n",
            sizeof(r->warnings) - strlen(r->warnings) - 1);
    r->all_checks_passed = 0;
  } else {
    r->running_bare_metal = 1;
  }

  /* ---- SMT ---- */
  char smt[16] = {0};
  benchmon_read_sysfs_str("/sys/devices/system/cpu/smt/active", smt,
                          sizeof(smt));
  r->smt_disabled = (smt[0] == '0') ? 1 : 0;
  if (!r->smt_disabled) {
    strncat(r->warnings,
            "WARNING: SMT is enabled — sibling-core interference\n",
            sizeof(r->warnings) - strlen(r->warnings) - 1);
    r->all_checks_passed = 0;
  }

  /* Threads per core */
  char tpc[16] = {0};
  if (benchmon_exec("lscpu | grep 'Thread(s) per core' | awk '{print $NF}'",
                    tpc, sizeof(tpc)) == 0)
    r->threads_per_core = atoi(tpc);

  /* ---- Isolated cores ---- */
  char iso[256] = {0};
  if (benchmon_read_sysfs_str("/sys/devices/system/cpu/isolated", iso,
                              sizeof(iso)) == 0 &&
      strlen(iso) > 0) {
    r->cores_isolated = 1;
    char *tok = strtok(iso, ",");
    while (tok && r->isolated_core_count < 64) {
      int a, b;
      if (sscanf(tok, "%d-%d", &a, &b) == 2) {
        for (int i = a; i <= b && r->isolated_core_count < 64; i++)
          r->isolated_core_list[r->isolated_core_count++] = i;
      } else {
        r->isolated_core_list[r->isolated_core_count++] = atoi(tok);
      }
      tok = strtok(NULL, ",");
    }
  } else {
    strncat(r->warnings, "WARNING: No CPU cores isolated (isolcpus not set)\n",
            sizeof(r->warnings) - strlen(r->warnings) - 1);
    r->all_checks_passed = 0;
  }

  /* ---- nohz_full ---- */
  char cmdline[2048] = {0};
  benchmon_read_sysfs_str("/proc/cmdline", cmdline, sizeof(cmdline));
  r->nohz_full_active = (strstr(cmdline, "nohz_full=") != NULL) ? 1 : 0;
  if (!r->nohz_full_active) {
    strncat(r->warnings, "WARNING: nohz_full not active — timer tick jitter\n",
            sizeof(r->warnings) - strlen(r->warnings) - 1);
  }

  /* ---- Frequency boost ---- */
  long no_turbo = 0;
  if (benchmon_read_sysfs_int("/sys/devices/system/cpu/intel_pstate/no_turbo",
                              &no_turbo) == 0)
    r->frequency_boost_off = (no_turbo == 1);
  else {
    long boost = 1;
    if (benchmon_read_sysfs_int("/sys/devices/system/cpu/cpufreq/boost",
                                &boost) == 0)
      r->frequency_boost_off = (boost == 0);
  }

  /* Actual frequency */
  char freq[32] = {0};
  if (benchmon_exec("cat /proc/cpuinfo | grep 'cpu MHz' | head -1 "
                    "| awk '{print $NF}'",
                    freq, sizeof(freq)) == 0)
    r->actual_freq_mhz = (int)atof(freq);

  /* ---- Memory ---- */
  struct sysinfo si;
  sysinfo(&si);
  r->total_ram_mb = (si.totalram * si.mem_unit) / (1024 * 1024);
  r->available_ram_mb = (si.freeram * si.mem_unit) / (1024 * 1024);
  r->swap_off = (si.totalswap == 0) ? 1 : 0;
  if (!r->swap_off) {
    strncat(r->warnings,
            "WARNING: Swap is enabled — risk of page-out latency\n",
            sizeof(r->warnings) - strlen(r->warnings) - 1);
  }

  /* ---- Network namespaces (dynamic names) ---- */
  char ns_list[512] = {0};
  benchmon_exec("ip netns list 2>/dev/null", ns_list, sizeof(ns_list));
  r->ns_server_exists = (strstr(ns_list, ns_s) != NULL);
  r->ns_client_exists = (strstr(ns_list, ns_c) != NULL);

  /* ---- veth + offloading (dynamic names) ---- */
  if (r->ns_server_exists) {
    char eth_cmd[256], eth_out[256] = {0};
    snprintf(eth_cmd, sizeof(eth_cmd),
             "ip netns exec %s ethtool -k %s 2>/dev/null "
             "| grep 'generic-segmentation-offload'",
             ns_s, ve_s);
    benchmon_exec(eth_cmd, eth_out, sizeof(eth_out));
    r->offloading_disabled = (strstr(eth_out, "off") != NULL);

    char tc_cmd[256], tc_out[256] = {0};
    snprintf(tc_cmd, sizeof(tc_cmd),
             "ip netns exec %s tc qdisc show 2>/dev/null", ns_s);
    benchmon_exec(tc_cmd, tc_out, sizeof(tc_out));
    r->netem_active = (strstr(tc_out, "netem") != NULL);
    r->veth_link_up = (strstr(tc_out, "veth") != NULL) || r->ns_server_exists;
  }

  /* ---- irqbalance ---- */
  char irqb[64] = {0};
  benchmon_exec("systemctl is-active irqbalance 2>/dev/null", irqb,
                sizeof(irqb));
  r->irqbalance_stopped =
      (strcmp(irqb, "inactive") == 0 || strcmp(irqb, "unknown") == 0);

  return BENCHMON_OK;
}

/* ------------------------------------------------------------------ */
/*  Public: benchmon_report()                                          */
/* ------------------------------------------------------------------ */

benchmon_status_t benchmon_report(int fd) {
  benchmon_verify_result_t v;
  benchmon_verify(&v, NULL); /* NULL = read from active config */

  dprintf(fd,
          "╔══════════════════════════════════════════╗\n"
          "║        BENCHMON SYSTEM REPORT            ║\n"
          "╠══════════════════════════════════════════╣\n"
          "║ Kernel:    %-28s ║\n"
          "║ CPU:       %-28s ║\n"
          "║ Freq:      %-4d MHz                      ║\n"
          "║ RAM:       %-5lu / %-5lu MB               ║\n"
          "║ Platform:  %-28s ║\n"
          "╠══════════════════════════════════════════╣\n"
          "║ SMT:       %-28s ║\n"
          "║ Isolated:  %-28s ║\n"
          "║ nohz_full: %-28s ║\n"
          "║ Boost:     %-28s ║\n"
          "║ Swap:      %-28s ║\n"
          "║ irqbal:    %-28s ║\n"
          "║ Netns:     server=%-3s client=%-3s         ║\n"
          "║ NetEm:     %-28s ║\n"
          "║ Offload:   %-28s ║\n"
          "╠══════════════════════════════════════════╣\n"
          "║ READY:     %-28s ║\n"
          "╚══════════════════════════════════════════╝\n",
          v.kernel_version, v.cpu_model, v.actual_freq_mhz,
          (unsigned long)v.available_ram_mb, (unsigned long)v.total_ram_mb,
          v.running_bare_metal ? "Bare metal" : v.hypervisor,
          v.smt_disabled ? "DISABLED ✓" : "ENABLED ✗",
          v.cores_isolated ? "YES ✓" : "NO ✗",
          v.nohz_full_active ? "ACTIVE ✓" : "INACTIVE ✗",
          v.frequency_boost_off ? "OFF ✓" : "ON ✗",
          v.swap_off ? "OFF ✓" : "ON ✗",
          v.irqbalance_stopped ? "STOPPED ✓" : "RUNNING ✗",
          v.ns_server_exists ? "yes" : "no", v.ns_client_exists ? "yes" : "no",
          v.netem_active ? "ACTIVE ✓" : "INACTIVE",
          v.offloading_disabled ? "OFF ✓" : "ON ✗",
          v.all_checks_passed ? "ALL CHECKS PASSED ✓" : "ISSUES FOUND ✗");

  if (v.warnings[0]) {
    dprintf(fd, "\n%s\n", v.warnings);
  }

  return BENCHMON_OK;
}

/* ------------------------------------------------------------------ */
/*  Kernel passthrough stubs (real impl in kmod)                       */
/* ------------------------------------------------------------------ */

int benchmon_net_passthru_available(void) {
  char buf[64] = {0};
  benchmon_exec("lsmod 2>/dev/null | grep benchmon_net", buf, sizeof(buf));
  return (strlen(buf) > 0) ? 1 : 0;
}

int benchmon_net_passthru_open(const char *iface) {
  (void)iface;
  BENCHMON_SET_ERR("Kernel passthrough module not loaded");
  return -1;
}

int benchmon_net_passthru_read(int fd, benchmon_packet_event_t *events,
                               int max) {
  (void)fd;
  (void)events;
  (void)max;
  return 0;
}

void benchmon_net_passthru_close(int fd) {
  if (fd >= 0)
    close(fd);
}