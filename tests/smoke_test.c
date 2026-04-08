/*
 * smoke_test.c — Quick validation that libbenchmon links and runs.
 * Does NOT require root (verify + monitor will just show partial data).
 *
 * Build:  gcc -O2 -I include -o smoke_test tests/smoke_test.c -L. -lbenchmon
 * Run:    LD_LIBRARY_PATH=. ./smoke_test
 */

#include "benchmon.h"
#include <stdio.h>
#include <string.h>

#define CHECK(label, cond) do { \
    if (cond) { printf("  ✓ %s\n", label); pass++; } \
    else      { printf("  ✗ %s\n", label); fail++; } \
} while (0)

int main(void) {
    int pass = 0, fail = 0;

    printf("benchmon smoke test\n");
    printf("═══════════════════\n\n");

    /* ---- Timestamp sanity ---- */
    {
        uint64_t t1 = benchmon_now_ns();
        uint64_t t2 = benchmon_now_ns();
        CHECK("benchmon_now_ns() returns increasing values", t2 >= t1);
        printf("    (delta: %lu ns)\n", (unsigned long)(t2 - t1));
    }

    /* ---- TSC sanity ---- */
    {
        uint64_t c1 = benchmon_rdtsc();
        uint64_t c2 = benchmon_rdtsc();
        CHECK("benchmon_rdtsc() returns increasing values", c2 >= c1);
    }

    /* ---- Verify ---- */
    {
        benchmon_verify_result_t v;
        benchmon_status_t rc = benchmon_verify(&v);
        CHECK("benchmon_verify() returns OK", rc == BENCHMON_OK);
        CHECK("kernel_version is populated",
              strlen(v.kernel_version) > 0);
        printf("    kernel: %s\n", v.kernel_version);
        printf("    cpu:    %s\n", v.cpu_model);
        printf("    freq:   %d MHz\n", v.actual_freq_mhz);
        printf("    ram:    %lu / %lu MB\n",
               (unsigned long)v.available_ram_mb,
               (unsigned long)v.total_ram_mb);
        printf("    bare-metal: %s\n",
               v.running_bare_metal ? "yes" : "no");
    }

    /* ---- Monitor init ---- */
    {
        benchmon_monitor_t *mon = benchmon_monitor_init(
            BENCHMON_MON_CPU | BENCHMON_MON_MEMORY |
            BENCHMON_MON_NETWORK | BENCHMON_MON_DISK,
            NULL, 0);
        CHECK("benchmon_monitor_init() returns non-NULL",
              mon != NULL);

        if (mon) {
            /* ---- Snapshot ---- */
            benchmon_snapshot_t snap;
            benchmon_status_t rc = benchmon_snapshot(mon, &snap);
            CHECK("benchmon_snapshot() returns OK", rc == BENCHMON_OK);
            CHECK("capture_latency_ns > 0",
                  snap.capture_latency_ns > 0);

            printf("    capture latency: %lu ns (%.1f μs)\n",
                   (unsigned long)snap.capture_latency_ns,
                   snap.capture_latency_ns / 1000.0);
            printf("    cpu cores seen:  %d\n", snap.cpu_count);
            printf("    net ifaces seen: %d\n", snap.net_count);
            printf("    disk devs seen:  %d\n", snap.disk_count);

            if (snap.mem.total_bytes > 0) {
                printf("    memory used:     %lu / %lu MB\n",
                       (unsigned long)(snap.mem.used_bytes / (1024*1024)),
                       (unsigned long)(snap.mem.total_bytes / (1024*1024)));
            }

            /* Multiple snapshots for timing */
            {
                const int N = 100;
                uint64_t start = benchmon_now_ns();
                for (int i = 0; i < N; i++) {
                    benchmon_snapshot(mon, &snap);
                }
                uint64_t elapsed = benchmon_now_ns() - start;
                double avg_us = (double)elapsed / N / 1000.0;
                printf("    avg snapshot time (%d samples): %.1f μs\n",
                       N, avg_us);
                CHECK("avg snapshot < 100 μs", avg_us < 100.0);
            }

            benchmon_monitor_destroy(mon);
            CHECK("benchmon_monitor_destroy() completed", 1);
        }
    }

    /* ---- Report ---- */
    printf("\n--- Full report ---\n");
    benchmon_report(1);  /* stdout = fd 1 */

    /* ---- Summary ---- */
    printf("\n═══════════════════\n");
    printf("Results: %d passed, %d failed\n", pass, fail);
    return fail > 0 ? 1 : 0;
}
