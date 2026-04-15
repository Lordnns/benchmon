/*
 * benchmon_internal.h — Shared internals (not part of public API)
 */

#ifndef BENCHMON_INTERNAL_H
#define BENCHMON_INTERNAL_H

#define _GNU_SOURCE
#include "../Include/benchmon.h"
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

/* ------------------------------------------------------------------ */
/*  Thread-local error message                                         */
/* ------------------------------------------------------------------ */
#define BENCHMON_ERRBUF_SIZE 512
extern __thread char benchmon_errbuf[BENCHMON_ERRBUF_SIZE];

#define BENCHMON_SET_ERR(fmt, ...)                                             \
  snprintf(benchmon_errbuf, BENCHMON_ERRBUF_SIZE, fmt, ##__VA_ARGS__)

#define BENCHMON_SET_ERR_ERRNO(msg)                                            \
  snprintf(benchmon_errbuf, BENCHMON_ERRBUF_SIZE, "%s: %s", msg,               \
           strerror(errno))

/* ------------------------------------------------------------------ */
/*  Monitor internals                                                  */
/* ------------------------------------------------------------------ */

/* perf_event file descriptors per core */
typedef struct {
  int core_id;
  int fd_instructions; /* PERF_COUNT_HW_INSTRUCTIONS */
  int fd_cycles;       /* PERF_COUNT_HW_CPU_CYCLES   */
  int fd_cache_misses; /* PERF_COUNT_HW_CACHE_MISSES */

  /* For CPU usage calculation */
  uint64_t prev_timestamp_ns;
  uint64_t prev_busy_ns;

  /* sysfs fd for frequency (kept open, pread on hot path) */
  int fd_freq;
} benchmon_core_ctx_t;

/* Network interface context */
typedef struct {
  char iface[16];
  int fd_stats; /* fd to /sys/class/net/<iface>/statistics/ */

  /* sysfs fds kept open for pread */
  int fd_rx_bytes;
  int fd_tx_bytes;
  int fd_rx_packets;
  int fd_tx_packets;
  int fd_rx_dropped;
  int fd_tx_dropped;
  int fd_rx_errors;
  int fd_tx_errors;

  /* Passthrough ring buffer (if kernel module loaded) */
  int passthru_fd;
  void *passthru_mmap;
  size_t passthru_mmap_size;
} benchmon_net_ctx_t;

/* Disk device context */
typedef struct {
  char device[32];
  int fd_stat; /* fd to /sys/block/<dev>/stat */
} benchmon_disk_ctx_t;

/* Main monitor struct */
struct benchmon_monitor {
  benchmon_mon_flags_t flags;

  /* CPU */
  benchmon_core_ctx_t cores[64];
  int core_count;

  /* Network */
  benchmon_net_ctx_t nets[8];
  int net_count;

  /* Disk */
  benchmon_disk_ctx_t disks[8];
  int disk_count;

  /* Pre-read buffer (avoids stack allocation variance) */
  char read_buf[256];
};

/* ------------------------------------------------------------------ */
/*  Helper: run a shell command, capture exit code                     */
/* ------------------------------------------------------------------ */
int benchmon_exec(const char *cmd, char *out, size_t out_size);

/* ------------------------------------------------------------------ */
/*  Helper: read an integer from a sysfs/procfs path                   */
/* ------------------------------------------------------------------ */
int benchmon_read_sysfs_int(const char *path, long *value);

/* ------------------------------------------------------------------ */
/*  Helper: read a string from a sysfs/procfs path                     */
/* ------------------------------------------------------------------ */
int benchmon_read_sysfs_str(const char *path, char *buf, size_t len);

/* ------------------------------------------------------------------ */
/*  Helper: write a string to a sysfs path                             */
/* ------------------------------------------------------------------ */
int benchmon_write_sysfs_str(const char *path, const char *value);

/* ------------------------------------------------------------------ */
/*  Helper: read uint64 via pread from an already-open fd              */
/*  HOT PATH — no open/close overhead                                  */
/* ------------------------------------------------------------------ */
static inline uint64_t benchmon_pread_uint64(int fd) {
  char buf[32];
  ssize_t n = pread(fd, buf, sizeof(buf) - 1, 0);
  if (n <= 0)
    return 0;
  buf[n] = '\0';
  return (uint64_t)strtoull(buf, NULL, 10);
}

#endif /* BENCHMON_INTERNAL_H */
