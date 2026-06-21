//! Runtime metrics snapshot model and Prometheus exposition formatting.
//!
//! Kept dependency-free on purpose: the Prometheus 0.0.4 text format is simple
//! enough to emit by hand, and pulling in the `prometheus` crate would
//! contradict LightSandbox's lightweight stance. Counters are accumulated by
//! the runtime implementation (see `MetricsCollector` in
//! `lightsandbox-runtime-local`) and snapshotted here for serialization.

/// Fixed upper bounds (in milliseconds) for the `exec` duration histogram,
/// ascending. The implicit final bucket is `+Inf`. The values span typical
/// millisecond-scale agent commands up to longer-running scripts.
pub const EXEC_BUCKETS_MILLIS: &[u64] = &[10, 50, 100, 250, 500, 1000, 2500, 5000, 10000];

/// Exposition labels (in seconds) for [`EXEC_BUCKETS_MILLIS`], in the same
/// order. Used only when rendering text format.
const EXEC_BUCKETS_LABELS: &[&str] = &["0.01", "0.05", "0.1", "0.25", "0.5", "1", "2.5", "5", "10"];

/// Point-in-time snapshot of runtime-wide counters and gauges, surfaced via
/// [`crate::runtime::SandboxRuntime::metrics`] and the `GET /metrics` endpoint.
/// All fields are monotonic counters unless noted.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MetricsSnapshot {
    /// Total sandboxes created since the runtime started.
    pub sandboxes_created_total: u64,
    /// Sandboxes currently tracked by the runtime (gauge, not monotonic).
    pub sandboxes_active: u64,
    /// Total sandboxes explicitly removed via `remove`.
    pub sandboxes_removed_total: u64,
    /// Total `exec` calls that completed (normally or by timeout).
    pub exec_total: u64,
    /// `exec` calls that hit their timeout (a subset of `exec_total`).
    pub exec_timed_out_total: u64,
    /// Wall-clock duration distribution of `exec` calls.
    pub exec_duration: HistogramSnapshot,
    /// Total `cleanup_expired` invocations.
    pub gc_runs_total: u64,
    /// Total sandboxes reaped by `cleanup_expired`.
    pub gc_removed_total: u64,
    /// Total successful `write_file` calls.
    pub file_writes_total: u64,
    /// Total successful `read_file` calls.
    pub file_reads_total: u64,
}

/// Histogram snapshot for a single observed distribution.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HistogramSnapshot {
    /// Total number of observations (equals the `+Inf` bucket).
    pub count: u64,
    /// Sum of all observations, in milliseconds.
    pub sum_millis: u64,
    /// Cumulative counts, ascending: one per entry in [`EXEC_BUCKETS_MILLIS`]
    /// followed by the `+Inf` bucket. Each entry counts observations whose
    /// value is `<=` the corresponding upper bound.
    pub bucket_counts: Vec<u64>,
}

/// Renders `snap` as a Prometheus 0.0.4 text exposition.
pub fn format_prometheus(snap: &MetricsSnapshot) -> String {
    let mut out = String::with_capacity(2048);
    counter(
        &mut out,
        "lightsandbox_sandboxes_created_total",
        "Total sandboxes created since the runtime started.",
        snap.sandboxes_created_total,
    );
    gauge(
        &mut out,
        "lightsandbox_sandboxes_active",
        "Sandboxes currently tracked by the runtime.",
        snap.sandboxes_active,
    );
    counter(
        &mut out,
        "lightsandbox_sandboxes_removed_total",
        "Total sandboxes explicitly removed.",
        snap.sandboxes_removed_total,
    );
    counter(
        &mut out,
        "lightsandbox_exec_total",
        "Total exec calls that completed (normally or by timeout).",
        snap.exec_total,
    );
    counter(
        &mut out,
        "lightsandbox_exec_timed_out_total",
        "Exec calls that hit their timeout (subset of exec_total).",
        snap.exec_timed_out_total,
    );
    histogram(
        &mut out,
        "lightsandbox_exec_duration_seconds",
        "Exec wall-clock duration in seconds.",
        &snap.exec_duration,
    );
    counter(
        &mut out,
        "lightsandbox_gc_runs_total",
        "Total cleanup_expired invocations.",
        snap.gc_runs_total,
    );
    counter(
        &mut out,
        "lightsandbox_gc_removed_total",
        "Total sandboxes reaped by cleanup_expired.",
        snap.gc_removed_total,
    );
    counter(
        &mut out,
        "lightsandbox_file_writes_total",
        "Total successful write_file calls.",
        snap.file_writes_total,
    );
    counter(
        &mut out,
        "lightsandbox_file_reads_total",
        "Total successful read_file calls.",
        snap.file_reads_total,
    );
    out
}

fn counter(out: &mut String, name: &str, help: &str, value: u64) {
    out.push_str("# HELP ");
    out.push_str(name);
    out.push(' ');
    out.push_str(help);
    out.push('\n');
    out.push_str("# TYPE ");
    out.push_str(name);
    out.push_str(" counter\n");
    out.push_str(name);
    out.push(' ');
    out.push_str(&value.to_string());
    out.push('\n');
}

fn gauge(out: &mut String, name: &str, help: &str, value: u64) {
    out.push_str("# HELP ");
    out.push_str(name);
    out.push(' ');
    out.push_str(help);
    out.push('\n');
    out.push_str("# TYPE ");
    out.push_str(name);
    out.push_str(" gauge\n");
    out.push_str(name);
    out.push(' ');
    out.push_str(&value.to_string());
    out.push('\n');
}

fn histogram(out: &mut String, name: &str, help: &str, h: &HistogramSnapshot) {
    out.push_str("# HELP ");
    out.push_str(name);
    out.push(' ');
    out.push_str(help);
    out.push('\n');
    out.push_str("# TYPE ");
    out.push_str(name);
    out.push_str(" histogram\n");
    for (i, le) in EXEC_BUCKETS_LABELS.iter().enumerate() {
        let c = h.bucket_counts.get(i).copied().unwrap_or(0);
        out.push_str(name);
        out.push_str("_bucket{le=\"");
        out.push_str(le);
        out.push_str("\"} ");
        out.push_str(&c.to_string());
        out.push('\n');
    }
    let inf = h
        .bucket_counts
        .get(EXEC_BUCKETS_LABELS.len())
        .copied()
        .unwrap_or(0);
    out.push_str(name);
    out.push_str("_bucket{le=\"+Inf\"} ");
    out.push_str(&inf.to_string());
    out.push('\n');
    out.push_str(name);
    out.push_str("_sum ");
    out.push_str(&format_secs(h.sum_millis));
    out.push('\n');
    out.push_str(name);
    out.push_str("_count ");
    out.push_str(&h.count.to_string());
    out.push('\n');
}

/// Formats a millisecond sum as a seconds value with up to 3 decimals,
/// trimming trailing zeros (e.g. `1.5`, `0.025`, `2`).
fn format_secs(millis: u64) -> String {
    let secs = millis / 1000;
    let ms = millis % 1000;
    if ms == 0 {
        return secs.to_string();
    }
    let mut s = format!("{secs}.{ms:03}");
    while s.ends_with('0') {
        s.pop();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_emits_required_prometheus_stanzas() {
        let snap = MetricsSnapshot {
            sandboxes_created_total: 3,
            sandboxes_active: 1,
            exec_total: 2,
            exec_timed_out_total: 1,
            exec_duration: HistogramSnapshot {
                count: 2,
                sum_millis: 1500,
                bucket_counts: vec![0, 0, 1, 1, 1, 1, 1, 1, 1, 2],
            },
            ..MetricsSnapshot::default()
        };
        let text = format_prometheus(&snap);
        assert!(text.contains("# HELP lightsandbox_sandboxes_created_total"));
        assert!(text.contains("# TYPE lightsandbox_sandboxes_active gauge"));
        assert!(text.contains("lightsandbox_sandboxes_created_total 3"));
        assert!(text.contains("lightsandbox_exec_duration_seconds_bucket{le=\"+Inf\"} 2"));
        assert!(text.contains("lightsandbox_exec_duration_seconds_sum 1.5"));
        assert!(text.contains("lightsandbox_exec_duration_seconds_count 2"));
    }

    #[test]
    fn format_secs_trims_trailing_zeros() {
        assert_eq!(format_secs(0), "0");
        assert_eq!(format_secs(1500), "1.5");
        assert_eq!(format_secs(25), "0.025");
        assert_eq!(format_secs(2000), "2");
    }

    #[test]
    fn empty_snapshot_renders_zero_histogram() {
        let text = format_prometheus(&MetricsSnapshot::default());
        assert!(text.contains("lightsandbox_exec_duration_seconds_bucket{le=\"+Inf\"} 0"));
        assert!(text.contains("lightsandbox_exec_duration_seconds_sum 0"));
    }
}
