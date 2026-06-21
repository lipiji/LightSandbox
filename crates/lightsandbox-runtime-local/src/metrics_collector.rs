//! Lock-free accumulator for runtime metrics, backing
//! `SandboxRuntime::metrics()` on `LocalProcessRuntime`.
//!
//! Every field is a `Relaxed` atomic: counters are informational and never
//! gate correctness, so we trade sequential consistency for throughput under
//! the high-concurrency workloads LightSandbox targets. A snapshot is a
//! best-effort, non-atomic read of all counters at once.

use std::sync::atomic::{AtomicU64, Ordering};

use lightsandbox_core::metrics::{HistogramSnapshot, MetricsSnapshot, EXEC_BUCKETS_MILLIS};

pub struct MetricsCollector {
    sandboxes_created_total: AtomicU64,
    sandboxes_removed_total: AtomicU64,
    exec_total: AtomicU64,
    exec_timed_out_total: AtomicU64,
    exec_sum_millis: AtomicU64,
    /// One slot per bucket in `EXEC_BUCKETS_MILLIS`, plus the trailing `+Inf`.
    exec_buckets: Vec<AtomicU64>,
    gc_runs_total: AtomicU64,
    gc_removed_total: AtomicU64,
    file_writes_total: AtomicU64,
    file_reads_total: AtomicU64,
}

impl MetricsCollector {
    pub fn new() -> Self {
        // +1 for the implicit +Inf bucket.
        let exec_buckets = (0..EXEC_BUCKETS_MILLIS.len() + 1)
            .map(|_| AtomicU64::new(0))
            .collect();
        Self {
            sandboxes_created_total: AtomicU64::new(0),
            sandboxes_removed_total: AtomicU64::new(0),
            exec_total: AtomicU64::new(0),
            exec_timed_out_total: AtomicU64::new(0),
            exec_sum_millis: AtomicU64::new(0),
            exec_buckets,
            gc_runs_total: AtomicU64::new(0),
            gc_removed_total: AtomicU64::new(0),
            file_writes_total: AtomicU64::new(0),
            file_reads_total: AtomicU64::new(0),
        }
    }

    pub fn record_create(&self) {
        self.sandboxes_created_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_remove(&self) {
        self.sandboxes_removed_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_file_write(&self) {
        self.file_writes_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_file_read(&self) {
        self.file_reads_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_gc_run(&self) {
        self.gc_runs_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_gc_removed(&self, n: u64) {
        if n > 0 {
            self.gc_removed_total.fetch_add(n, Ordering::Relaxed);
        }
    }

    /// Records one `exec` observation: increments the total, accumulates the
    /// duration into the sum, optionally marks a timeout, and increments every
    /// histogram bucket whose upper bound is `>= duration_millis` (cumulative
    /// Prometheus convention), including `+Inf`.
    pub fn record_exec(&self, duration_millis: u64, timed_out: bool) {
        self.exec_total.fetch_add(1, Ordering::Relaxed);
        self.exec_sum_millis
            .fetch_add(duration_millis, Ordering::Relaxed);
        if timed_out {
            self.exec_timed_out_total.fetch_add(1, Ordering::Relaxed);
        }
        for (i, &bound) in EXEC_BUCKETS_MILLIS.iter().enumerate() {
            if duration_millis <= bound {
                self.exec_buckets[i].fetch_add(1, Ordering::Relaxed);
            }
        }
        let inf = self.exec_buckets.len() - 1;
        self.exec_buckets[inf].fetch_add(1, Ordering::Relaxed);
    }

    /// Builds a snapshot. `active` is the current sandbox count, supplied by
    /// the runtime (the collector itself does not track live sandboxes).
    pub fn snapshot(&self, active: u64) -> MetricsSnapshot {
        let bucket_counts: Vec<u64> = self
            .exec_buckets
            .iter()
            .map(|a| a.load(Ordering::Relaxed))
            .collect();
        let exec_total = self.exec_total.load(Ordering::Relaxed);
        MetricsSnapshot {
            sandboxes_created_total: self.sandboxes_created_total.load(Ordering::Relaxed),
            sandboxes_active: active,
            sandboxes_removed_total: self.sandboxes_removed_total.load(Ordering::Relaxed),
            exec_total,
            exec_timed_out_total: self.exec_timed_out_total.load(Ordering::Relaxed),
            exec_duration: HistogramSnapshot {
                count: exec_total,
                sum_millis: self.exec_sum_millis.load(Ordering::Relaxed),
                bucket_counts,
            },
            gc_runs_total: self.gc_runs_total.load(Ordering::Relaxed),
            gc_removed_total: self.gc_removed_total.load(Ordering::Relaxed),
            file_writes_total: self.file_writes_total.load(Ordering::Relaxed),
            file_reads_total: self.file_reads_total.load(Ordering::Relaxed),
        }
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_histogram_is_cumulative() {
        let c = MetricsCollector::new();
        // 50ms observation: buckets >= 0.05s (50ms) and up should increment.
        c.record_exec(50, false);
        // 2s observation: only buckets >= 2s.
        c.record_exec(2000, false);
        let snap = c.snapshot(0);
        let b = &snap.exec_duration.bucket_counts;
        // EXEC_BUCKETS_MILLIS = [10,50,100,250,500,1000,2500,5000,10000]
        // index 1 (50)   -> 1 (only the 50ms obs)
        // index 6 (2500) -> 2 (both obs: 50ms and 2000ms)
        // +Inf (index 9) -> 2
        assert_eq!(b[0], 0); // le=0.01
        assert_eq!(b[1], 1); // le=0.05
        assert_eq!(b[6], 2); // le=2.5
        assert_eq!(b[9], 2); // +Inf
        assert_eq!(snap.exec_duration.count, 2);
        assert_eq!(snap.exec_duration.sum_millis, 2050);
    }

    #[test]
    fn counters_accumulate_independently() {
        let c = MetricsCollector::new();
        c.record_create();
        c.record_create();
        c.record_remove();
        c.record_file_write();
        c.record_file_read();
        c.record_file_read();
        c.record_gc_run();
        c.record_gc_removed(3);
        let snap = c.snapshot(5);
        assert_eq!(snap.sandboxes_created_total, 2);
        assert_eq!(snap.sandboxes_removed_total, 1);
        assert_eq!(snap.sandboxes_active, 5);
        assert_eq!(snap.file_writes_total, 1);
        assert_eq!(snap.file_reads_total, 2);
        assert_eq!(snap.gc_runs_total, 1);
        assert_eq!(snap.gc_removed_total, 3);
    }
}
