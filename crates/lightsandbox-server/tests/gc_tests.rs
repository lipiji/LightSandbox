//! Integration test for the server's background GC loop (`gc.rs`).
//!
//! `cleanup_expired()` is exercised directly in the runtime-local tests, but
//! the wiring that actually drives it on a timer — `gc::spawn` and its
//! interval loop — had no coverage until this test. It starts that loop
//! against a real runtime and confirms an expired sandbox is reaped (and its
//! workspace removed) with no direct `cleanup_expired` call from the test.

use std::sync::Arc;
use std::time::{Duration, Instant};

use lightsandbox_core::{ResourceLimits, RuntimeConfig, SandboxRuntime, SandboxSpec};
use lightsandbox_runtime_local::LocalProcessRuntime;
use lightsandbox_server::{gc, state::AppState};

fn unique_tmp(name: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("lightsandbox_gc_{name}_{nanos:x}"))
}

#[tokio::test]
async fn gc_background_loop_reaps_expired_sandbox() {
    let root = unique_tmp("reap");
    let config = RuntimeConfig {
        workspace_root: root.clone(),
        limits: ResourceLimits::default(),
        remove_expired: true,
        ..RuntimeConfig::default()
    };
    let runtime = Arc::new(LocalProcessRuntime::new(config).expect("runtime builds"));
    let state = Arc::new(AppState {
        runtime: runtime.clone(),
    });

    // 1s interval, enabled. The first tick completes immediately (nothing
    // expired yet); subsequent ticks fire every 1s on the test's runtime.
    gc::spawn(state, 1, true);

    let info = runtime
        .create(SandboxSpec {
            ttl_seconds: Some(1),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(
        runtime
            .list()
            .await
            .unwrap()
            .iter()
            .any(|s| s.id == info.id),
        "sandbox should exist right after create"
    );

    // Poll until the GC loop reaps it. ttl=1s with a 1s interval => ~1-2s in
    // practice; allow 8s for a slow/loaded CI machine.
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if Instant::now() > deadline {
            panic!("GC did not reap the expired sandbox within 8s");
        }
        let reaped = !runtime
            .list()
            .await
            .unwrap()
            .iter()
            .any(|s| s.id == info.id);
        if reaped {
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    // remove_expired = true, so the workspace directory must be gone too —
    // not just the in-memory entry.
    assert!(
        !root.join(&info.id).exists(),
        "GC should have removed the workspace directory too"
    );
}

#[tokio::test]
async fn gc_disabled_leaves_expired_sandbox_in_place() {
    // The `enabled = false` branch of `gc::spawn` returns without spawning a
    // loop. An expired sandbox must therefore stay in `list()` (it is still
    // tracked, just never reaped) — guards against a regression that drops
    // the early return and always spawns.
    let root = unique_tmp("disabled");
    let config = RuntimeConfig {
        workspace_root: root.clone(),
        limits: ResourceLimits::default(),
        remove_expired: true,
        ..RuntimeConfig::default()
    };
    let runtime = Arc::new(LocalProcessRuntime::new(config).expect("runtime builds"));
    let state = Arc::new(AppState {
        runtime: runtime.clone(),
    });

    gc::spawn(state, 1, false);

    let info = runtime
        .create(SandboxSpec {
            ttl_seconds: Some(1),
            ..Default::default()
        })
        .await
        .unwrap();

    // Wait well past the ttl + one interval, then confirm the sandbox is
    // still listed — no background reaping happened.
    tokio::time::sleep(Duration::from_millis(2500)).await;
    let still_listed = runtime
        .list()
        .await
        .unwrap()
        .iter()
        .any(|s| s.id == info.id);
    assert!(
        still_listed,
        "with GC disabled, an expired sandbox must not be reaped"
    );

    // And a direct cleanup_expired call still works (the runtime itself is
    // unaffected) — proving the sandbox was merely un-reaped, not immortal.
    let processed = runtime.cleanup_expired().await.unwrap();
    assert!(processed >= 1, "manual cleanup should still reap it");
}
