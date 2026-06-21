//! Concurrency sanity-check / benchmark for `LocalProcessRuntime`.
//!
//! Spins up `--n` sandboxes with at most `--concurrency` in flight at once,
//! runs a trivial command in each, then removes them. Reports measured
//! timings for this run only — v0.1 makes no fabricated performance claims,
//! per the project spec. Run it yourself to benchmark your own environment:
//!
//! ```bash
//! cargo run -p lightsandbox-server --example concurrent_sandboxes -- --n 20 --concurrency 5
//! ```

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use clap::Parser;
use lightsandbox_core::{ExecRequest, ResourceLimits, RuntimeConfig, SandboxRuntime, SandboxSpec};
use lightsandbox_runtime_local::LocalProcessRuntime;
use tokio::sync::Semaphore;

#[derive(Parser, Debug)]
#[command(name = "concurrent_sandboxes")]
struct Args {
    /// Total number of sandboxes to create and exercise.
    #[arg(long, default_value_t = 20)]
    n: usize,

    /// Maximum number of sandboxes being created/executed/removed at once.
    #[arg(long, default_value_t = 5)]
    concurrency: usize,
}

#[cfg(windows)]
const TRIVIAL_CMD: &str = "echo hi";
#[cfg(not(windows))]
const TRIVIAL_CMD: &str = "echo hi";

#[tokio::main]
async fn main() {
    let args = Args::parse();

    let workspace_root =
        std::env::temp_dir().join(format!("lightsandbox-concurrent-{}", std::process::id()));
    std::fs::create_dir_all(&workspace_root).expect("failed to create temp workspace_root");

    let config = RuntimeConfig {
        workspace_root: workspace_root.clone(),
        limits: ResourceLimits {
            max_sandboxes: args.n + 1,
            max_concurrent_exec: args.concurrency,
            ..ResourceLimits::default()
        },
        ..RuntimeConfig::default()
    };

    let runtime =
        Arc::new(LocalProcessRuntime::new(config).expect("failed to build local runtime"));
    let pipeline_limit = Arc::new(Semaphore::new(args.concurrency));

    let succeeded = Arc::new(AtomicUsize::new(0));
    let failed = Arc::new(AtomicUsize::new(0));

    println!(
        "Running {} sandboxes with concurrency {} (workspace_root: {})",
        args.n,
        args.concurrency,
        workspace_root.display()
    );

    let overall_start = Instant::now();
    let mut handles = Vec::with_capacity(args.n);

    for i in 0..args.n {
        let runtime = runtime.clone();
        let pipeline_limit = pipeline_limit.clone();
        let succeeded = succeeded.clone();
        let failed = failed.clone();

        handles.push(tokio::spawn(async move {
            let _permit = pipeline_limit.acquire().await.expect("semaphore closed");
            let task_start = Instant::now();

            let result = run_one(&*runtime, i).await;

            let elapsed = task_start.elapsed();
            match result {
                Ok(()) => {
                    succeeded.fetch_add(1, Ordering::Relaxed);
                }
                Err(e) => {
                    failed.fetch_add(1, Ordering::Relaxed);
                    eprintln!("sandbox task {i} failed after {elapsed:?}: {e}");
                }
            }
            elapsed
        }));
    }

    let mut durations = Vec::with_capacity(args.n);
    for handle in handles {
        durations.push(handle.await.expect("task panicked"));
    }

    let overall_elapsed = overall_start.elapsed();
    let succeeded = succeeded.load(Ordering::Relaxed);
    let failed = failed.load(Ordering::Relaxed);

    durations.sort();
    let min = durations.first().copied().unwrap_or_default();
    let max = durations.last().copied().unwrap_or_default();
    let total: std::time::Duration = durations.iter().sum();
    let avg = if durations.is_empty() {
        std::time::Duration::ZERO
    } else {
        total / durations.len() as u32
    };

    println!("--- results ---");
    println!("succeeded: {succeeded}");
    println!("failed: {failed}");
    println!("total wall time: {overall_elapsed:?}");
    println!("per-sandbox: min={min:?} avg={avg:?} max={max:?}");

    let remaining = std::fs::read_dir(&workspace_root)
        .map(|entries| entries.count())
        .unwrap_or(0);
    println!("workspace dirs remaining on disk: {remaining}");

    let _ = std::fs::remove_dir_all(&workspace_root);
}

async fn run_one(runtime: &dyn SandboxRuntime, _index: usize) -> Result<(), String> {
    let info = runtime
        .create(SandboxSpec::default())
        .await
        .map_err(|e| format!("create failed: {e}"))?;

    let exec_result = runtime
        .exec(
            &info.id,
            ExecRequest {
                cmd: TRIVIAL_CMD.to_string(),
                timeout_seconds: Some(10),
                env: None,
            },
        )
        .await
        .map_err(|e| format!("exec failed: {e}"))?;

    if exec_result.exit_code != 0 || exec_result.timed_out {
        return Err(format!(
            "unexpected exec result: exit_code={} timed_out={}",
            exec_result.exit_code, exec_result.timed_out
        ));
    }

    runtime
        .remove(&info.id)
        .await
        .map_err(|e| format!("remove failed: {e}"))?;

    Ok(())
}
