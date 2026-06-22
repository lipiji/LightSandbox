use std::sync::Arc;
use std::time::Duration;

use lightsandbox_core::{
    ExecOutputEvent, ExecRequest, LightSandboxError, ResourceLimits, RuntimeConfig, SandboxRuntime,
    SandboxSpec,
};
use lightsandbox_runtime_local::LocalProcessRuntime;

fn test_limits() -> ResourceLimits {
    ResourceLimits {
        max_sandboxes: 1000,
        max_concurrent_exec: 50,
        default_ttl_seconds: 600,
        default_exec_timeout_seconds: 5,
        max_stdout_bytes: 4096,
        max_stderr_bytes: 4096,
        max_file_size_bytes: 1024,
        max_read_file_bytes: 1024,
    }
}

fn test_runtime(root: &std::path::Path) -> LocalProcessRuntime {
    let config = RuntimeConfig {
        workspace_root: root.to_path_buf(),
        limits: test_limits(),
        allow_absolute_paths: false,
        allow_path_traversal: false,
        hide_host_paths: true,
        remove_expired: true,
        templates_dir: None,
        pool_enabled: false,
        pool_min_idle: 0,
        persistence_db_path: None,
    };
    LocalProcessRuntime::new(config).expect("test runtime builds")
}

/// Like `test_runtime`, but with caller-supplied limits — for tests that need
/// a specific `max_sandboxes`, output cap, or read/write size cap.
fn runtime_with_limits(root: &std::path::Path, limits: ResourceLimits) -> LocalProcessRuntime {
    let config = RuntimeConfig {
        workspace_root: root.to_path_buf(),
        limits,
        allow_absolute_paths: false,
        allow_path_traversal: false,
        hide_host_paths: true,
        remove_expired: true,
        templates_dir: None,
        pool_enabled: false,
        pool_min_idle: 0,
        persistence_db_path: None,
    };
    LocalProcessRuntime::new(config).expect("test runtime builds")
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("lightsandbox_test_{name}_{}", uuid_like()));
    dir
}

fn uuid_like() -> String {
    format!(
        "{:x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}

#[cfg(windows)]
fn echo_cmd(text: &str) -> String {
    format!("echo {text}")
}
#[cfg(not(windows))]
fn echo_cmd(text: &str) -> String {
    format!("echo {text}")
}

#[tokio::test]
async fn create_succeeds() {
    let root = temp_dir("create");
    let rt = test_runtime(&root);
    let info = rt.create(SandboxSpec::default()).await.unwrap();
    assert!(info.id.starts_with("sbx_"));
}

#[tokio::test]
async fn list_shows_created_sandbox() {
    let root = temp_dir("list");
    let rt = test_runtime(&root);
    let info = rt.create(SandboxSpec::default()).await.unwrap();
    let listed = rt.list().await.unwrap();
    assert!(listed.iter().any(|s| s.id == info.id));
}

#[tokio::test]
async fn exec_echo_succeeds() {
    let root = temp_dir("exec_echo");
    let rt = test_runtime(&root);
    let info = rt.create(SandboxSpec::default()).await.unwrap();
    let result = rt
        .exec(
            &info.id,
            ExecRequest {
                cmd: echo_cmd("hello"),
                timeout_seconds: None,
                env: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(result.stdout.contains("hello"));
    assert!(!result.timed_out);
}

/// Locates a real python.exe via PATH, skipping the Windows "App Execution
/// Alias" stub under WindowsApps (which exits 0 with no output if Python
/// isn't installed through the Store). PATH here is the native Windows
/// (`;`-separated) value seen by this process, even when invoked from a
/// Git Bash shell whose own `$PATH` is Unix-style.
#[cfg(windows)]
fn find_python() -> Option<String> {
    let path = std::env::var("PATH").unwrap_or_default();
    path.split(';')
        .filter(|p| !p.to_lowercase().contains("windowsapps"))
        .map(|dir| std::path::Path::new(dir).join("python.exe"))
        .find(|p| p.is_file())
        .map(|p| p.to_string_lossy().into_owned())
}

#[tokio::test]
async fn exec_python_succeeds() {
    let root = temp_dir("exec_python");
    let rt = test_runtime(&root);
    let info = rt.create(SandboxSpec::default()).await.unwrap();

    #[cfg(windows)]
    let cmd = match find_python() {
        Some(python) => format!("{python} -c print(1+1)"),
        None => {
            eprintln!("skipping exec_python_succeeds: no python.exe found on PATH");
            return;
        }
    };
    #[cfg(not(windows))]
    // The runtime hands `cmd` to `sh -c <cmd>` on Unix, so the python
    // argument must be quoted — bare `print(1+1)` makes `sh` choke on the
    // parens (`syntax error near unexpected token '('`). `cmd` on Windows
    // doesn't care, hence the unquoted form in the windows branch above.
    let cmd = "python3 -c \"print(1+1)\"".to_string();

    let result = rt
        .exec(
            &info.id,
            ExecRequest {
                cmd,
                timeout_seconds: None,
                env: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0, "stderr={}", result.stderr);
    assert!(result.stdout.contains('2'));
}

#[tokio::test]
async fn write_then_read_file_round_trips() {
    let root = temp_dir("write_read");
    let rt = test_runtime(&root);
    let info = rt.create(SandboxSpec::default()).await.unwrap();
    rt.write_file(&info.id, "hello.txt", b"hi there".to_vec())
        .await
        .unwrap();
    let content = rt.read_file(&info.id, "hello.txt").await.unwrap();
    assert_eq!(content, b"hi there");
}

#[tokio::test]
async fn exec_after_remove_is_rejected() {
    let root = temp_dir("remove_then_exec");
    let rt = test_runtime(&root);
    let info = rt.create(SandboxSpec::default()).await.unwrap();
    rt.remove(&info.id).await.unwrap();
    let result = rt
        .exec(
            &info.id,
            ExecRequest {
                cmd: echo_cmd("hi"),
                timeout_seconds: None,
                env: None,
            },
        )
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn extend_ttl_moves_expiry_forward() {
    let root = temp_dir("extend_ttl");
    let rt = test_runtime(&root);
    let info = rt
        .create(SandboxSpec {
            ttl_seconds: Some(10),
            ..Default::default()
        })
        .await
        .unwrap();
    let original_expiry = info.expires_at.unwrap();

    let updated = rt.extend_ttl(&info.id, 9999).await.unwrap();
    assert!(updated.expires_at.unwrap() > original_expiry);
}

#[tokio::test]
async fn extend_ttl_on_unknown_sandbox_is_rejected() {
    let root = temp_dir("extend_ttl_missing");
    let rt = test_runtime(&root);
    let result = rt.extend_ttl("sbx_does_not_exist", 60).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn exec_timeout_is_enforced() {
    let root = temp_dir("timeout");
    let rt = test_runtime(&root);
    let info = rt.create(SandboxSpec::default()).await.unwrap();
    let sleep_cmd = if cfg!(windows) {
        "ping -n 6 127.0.0.1 > NUL".to_string()
    } else {
        "sleep 6".to_string()
    };
    let result = rt
        .exec(
            &info.id,
            ExecRequest {
                cmd: sleep_cmd,
                timeout_seconds: Some(1),
                env: None,
            },
        )
        .await
        .unwrap();
    assert!(result.timed_out);
}

#[tokio::test]
async fn exec_stream_reports_chunks_and_done() {
    let root = temp_dir("exec_stream");
    let rt = Arc::new(test_runtime(&root));
    let info = rt.create(SandboxSpec::default()).await.unwrap();

    let (tx, mut rx) = tokio::sync::mpsc::channel(16);
    let rt2 = rt.clone();
    let id = info.id.clone();
    let handle = tokio::spawn(async move {
        rt2.exec_stream(
            &id,
            ExecRequest {
                cmd: echo_cmd("hello-stream"),
                timeout_seconds: None,
                env: None,
            },
            tx,
        )
        .await
    });

    let mut stdout = Vec::new();
    let mut done = None;
    while let Some(event) = rx.recv().await {
        match event {
            ExecOutputEvent::Stdout(chunk) => stdout.extend_from_slice(&chunk),
            ExecOutputEvent::Stderr(_) => {}
            ExecOutputEvent::Done {
                exit_code,
                timed_out,
                ..
            } => done = Some((exit_code, timed_out)),
            ExecOutputEvent::Error(msg) => panic!("unexpected error event: {msg}"),
        }
    }
    handle.await.unwrap().unwrap();

    let stdout = String::from_utf8_lossy(&stdout);
    assert!(stdout.contains("hello-stream"), "stdout was: {stdout}");
    assert_eq!(done, Some((0, false)));
}

#[tokio::test]
async fn exec_stream_timeout_is_enforced() {
    let root = temp_dir("exec_stream_timeout");
    let rt = Arc::new(test_runtime(&root));
    let info = rt.create(SandboxSpec::default()).await.unwrap();
    let sleep_cmd = if cfg!(windows) {
        "ping -n 6 127.0.0.1 > NUL".to_string()
    } else {
        "sleep 6".to_string()
    };

    let (tx, mut rx) = tokio::sync::mpsc::channel(16);
    let rt2 = rt.clone();
    let id = info.id.clone();
    let handle = tokio::spawn(async move {
        rt2.exec_stream(
            &id,
            ExecRequest {
                cmd: sleep_cmd,
                timeout_seconds: Some(1),
                env: None,
            },
            tx,
        )
        .await
    });

    let mut done = None;
    while let Some(event) = rx.recv().await {
        if let ExecOutputEvent::Done { timed_out, .. } = event {
            done = Some(timed_out);
        }
    }
    handle.await.unwrap().unwrap();

    assert_eq!(done, Some(true));
}

#[tokio::test]
async fn path_traversal_is_rejected() {
    let root = temp_dir("traversal");
    let rt = test_runtime(&root);
    let info = rt.create(SandboxSpec::default()).await.unwrap();
    let result = rt
        .write_file(&info.id, "../escape.txt", b"x".to_vec())
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn oversized_file_write_is_rejected() {
    let root = temp_dir("oversized");
    let rt = test_runtime(&root);
    let info = rt.create(SandboxSpec::default()).await.unwrap();
    let big = vec![0u8; 2048];
    let result = rt.write_file(&info.id, "big.bin", big).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn gc_cleans_expired_sandbox() {
    let root = temp_dir("gc");
    let rt = test_runtime(&root);
    let info = rt
        .create(SandboxSpec {
            ttl_seconds: Some(0),
            metadata: None,
            env: None,
            template: None,
        })
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    let removed = rt.cleanup_expired().await.unwrap();
    assert!(removed >= 1);
    let listed = rt.list().await.unwrap();
    assert!(!listed.iter().any(|s| s.id == info.id));
}

#[tokio::test]
async fn concurrent_create_does_not_crash() {
    let root = temp_dir("concurrent");
    let rt = std::sync::Arc::new(test_runtime(&root));
    let mut handles = Vec::new();
    for _ in 0..20 {
        let rt = rt.clone();
        handles.push(tokio::spawn(async move {
            rt.create(SandboxSpec::default()).await.unwrap()
        }));
    }
    let mut ids = std::collections::HashSet::new();
    for h in handles {
        let info = h.await.unwrap();
        ids.insert(info.id);
    }
    assert_eq!(ids.len(), 20);
    let listed = rt.list().await.unwrap();
    assert_eq!(listed.len(), 20);
}

#[tokio::test]
async fn metrics_reflect_operations() {
    let root = temp_dir("metrics");
    let rt = test_runtime(&root);

    // A fresh runtime reports zero counters and an empty (still well-formed)
    // histogram.
    let before = rt.metrics().await.unwrap();
    assert_eq!(before.sandboxes_created_total, 0);
    assert_eq!(before.sandboxes_active, 0);
    assert_eq!(before.exec_duration.count, 0);

    let info = rt.create(SandboxSpec::default()).await.unwrap();
    rt.write_file(&info.id, "a.txt", b"x".to_vec())
        .await
        .unwrap();
    rt.read_file(&info.id, "a.txt").await.unwrap();
    rt.exec(
        &info.id,
        ExecRequest {
            cmd: echo_cmd("hi"),
            timeout_seconds: None,
            env: None,
        },
    )
    .await
    .unwrap();
    rt.remove(&info.id).await.unwrap();
    let after = rt.metrics().await.unwrap();

    assert_eq!(after.sandboxes_created_total, 1);
    assert_eq!(after.sandboxes_active, 0); // removed
    assert_eq!(after.sandboxes_removed_total, 1);
    assert_eq!(after.exec_total, 1);
    assert_eq!(after.exec_timed_out_total, 0);
    assert_eq!(after.exec_duration.count, 1);
    assert!(after.exec_duration.sum_millis < 5_000);
    // The +Inf bucket must equal the total observation count.
    assert_eq!(
        *after.exec_duration.bucket_counts.last().unwrap(),
        after.exec_duration.count
    );
    assert_eq!(after.file_writes_total, 1);
    assert_eq!(after.file_reads_total, 1);
}

#[tokio::test]
async fn metrics_count_exec_timeout_separately() {
    let root = temp_dir("metrics_timeout");
    let rt = test_runtime(&root);
    let info = rt.create(SandboxSpec::default()).await.unwrap();
    let sleep_cmd = if cfg!(windows) {
        "ping -n 6 127.0.0.1 > NUL".to_string()
    } else {
        "sleep 6".to_string()
    };
    let result = rt
        .exec(
            &info.id,
            ExecRequest {
                cmd: sleep_cmd,
                timeout_seconds: Some(1),
                env: None,
            },
        )
        .await
        .unwrap();
    assert!(result.timed_out);

    let snap = rt.metrics().await.unwrap();
    assert_eq!(snap.exec_total, 1);
    assert_eq!(snap.exec_timed_out_total, 1);
}

fn runtime_with_templates(
    root: &std::path::Path,
    templates_dir: std::path::PathBuf,
) -> LocalProcessRuntime {
    let config = RuntimeConfig {
        workspace_root: root.to_path_buf(),
        limits: test_limits(),
        allow_absolute_paths: false,
        allow_path_traversal: false,
        hide_host_paths: true,
        remove_expired: true,
        templates_dir: Some(templates_dir),
        pool_enabled: false,
        pool_min_idle: 0,
        persistence_db_path: None,
    };
    LocalProcessRuntime::new(config).expect("test runtime builds")
}

#[tokio::test]
async fn create_with_template_populates_workspace() {
    let root = temp_dir("template");
    let templates_dir = root.join("templates");
    let tpl = templates_dir.join("demo");
    tokio::fs::create_dir_all(tpl.join("sub")).await.unwrap();
    tokio::fs::write(tpl.join("greet.txt"), b"hi from template")
        .await
        .unwrap();
    tokio::fs::write(tpl.join("sub/nested.txt"), b"nested")
        .await
        .unwrap();

    let rt = runtime_with_templates(&root, templates_dir);
    let info = rt
        .create(SandboxSpec {
            ttl_seconds: None,
            metadata: None,
            env: None,
            template: Some("demo".into()),
        })
        .await
        .unwrap();

    // Both the top-level file and the nested one were copied through.
    let top = rt.read_file(&info.id, "greet.txt").await.unwrap();
    assert_eq!(top, b"hi from template");
    let nested = rt.read_file(&info.id, "sub/nested.txt").await.unwrap();
    assert_eq!(nested, b"nested");
}

#[tokio::test]
async fn unknown_template_is_rejected() {
    let root = temp_dir("template_missing");
    let rt = runtime_with_templates(&root, root.join("templates"));
    let result = rt
        .create(SandboxSpec {
            ttl_seconds: None,
            metadata: None,
            env: None,
            template: Some("does-not-exist".into()),
        })
        .await;
    assert!(
        matches!(
            result,
            Err(lightsandbox_core::LightSandboxError::InvalidPath(_))
        ),
        "expected InvalidPath, got {result:?}"
    );
}

#[tokio::test]
async fn pool_reuses_idle_slots_and_stays_consistent() {
    let root = temp_dir("pool");
    let config = RuntimeConfig {
        workspace_root: root.to_path_buf(),
        limits: test_limits(),
        allow_absolute_paths: false,
        allow_path_traversal: false,
        hide_host_paths: true,
        remove_expired: true,
        templates_dir: None,
        pool_enabled: true,
        pool_min_idle: 2,
        persistence_db_path: None,
    };
    let rt = LocalProcessRuntime::new(config).expect("test runtime builds");
    rt.prewarm().await;

    // Several template-less creates: each reuses a pooled slot when one is
    // available, otherwise builds fresh. list() must reflect only handed-out
    // sandboxes (idle slots never leak in), and metrics must count exactly the
    // creates issued.
    let mut ids = Vec::new();
    for _ in 0..5 {
        let info = rt.create(SandboxSpec::default()).await.unwrap();
        ids.push(info.id);
        // Let the lazy replenish task make progress between iterations.
        tokio::task::yield_now().await;
    }

    let listed = rt.list().await.unwrap();
    assert_eq!(listed.len(), 5, "idle slots must not appear in list()");
    for id in &ids {
        assert!(listed.iter().any(|s| s.id == *id));
    }

    let snap = rt.metrics().await.unwrap();
    assert_eq!(snap.sandboxes_created_total, 5);
}

// --- persistence (restart-survival) ---

/// Builds a runtime whose metadata is mirrored to `db_path`. Reusing the same
/// `root` and `db_path` across two runtimes simulates a process restart:
/// runtime B's `new()` reopens the db and repopulates its in-memory state.
fn persistent_runtime(root: &std::path::Path, db_path: &std::path::Path) -> LocalProcessRuntime {
    let config = RuntimeConfig {
        workspace_root: root.to_path_buf(),
        limits: test_limits(),
        allow_absolute_paths: false,
        allow_path_traversal: false,
        hide_host_paths: true,
        remove_expired: true,
        templates_dir: None,
        pool_enabled: false,
        pool_min_idle: 0,
        persistence_db_path: Some(db_path.to_path_buf()),
    };
    LocalProcessRuntime::new(config).expect("persistent test runtime builds")
}

#[tokio::test]
async fn persistence_restores_sandbox_after_restart() {
    let root = temp_dir("persist_restore");
    let db_path = root.join("lightsandbox.redb");

    // Runtime A: create two sandboxes (one with env + metadata so we can
    // verify those round-trip too), then drop A to simulate a stop.
    let info_a = {
        let rt = persistent_runtime(&root, &db_path);
        let plain = rt.create(SandboxSpec::default()).await.unwrap();
        let rich = rt
            .create(SandboxSpec {
                ttl_seconds: Some(3600),
                metadata: Some([("owner".to_string(), "alice".to_string())].into()),
                env: Some([("FOO".to_string(), "bar".to_string())].into()),
                template: None,
            })
            .await
            .unwrap();
        (plain.id, rich.id)
    };
    // A is dropped here — only its on-disk metadata + workspace dirs remain.

    // Runtime B: same root + db. The two sandboxes must reappear.
    let rt = persistent_runtime(&root, &db_path);
    let listed = rt.list().await.unwrap();
    assert_eq!(listed.len(), 2, "both sandboxes should be restored");
    assert!(listed.iter().any(|s| s.id == info_a.0));
    assert!(listed.iter().any(|s| s.id == info_a.1));

    // The restored rich sandbox is fully usable: exec runs in its workspace,
    // and the persisted env is applied. (Shell var syntax differs by OS:
    // cmd.exe expands %FOO%, sh expands $FOO.)
    #[cfg(windows)]
    let echo_env = "echo %FOO%";
    #[cfg(not(windows))]
    let echo_env = "echo $FOO";
    let result = rt
        .exec(
            &info_a.1,
            ExecRequest {
                cmd: echo_env.to_string(),
                timeout_seconds: None,
                env: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(
        result.stdout.trim(),
        "bar",
        "persisted env should be applied to restored sandbox"
    );
}

#[tokio::test]
async fn persistence_skips_already_expired_sandbox() {
    let root = temp_dir("persist_expired");
    let db_path = root.join("lightsandbox.redb");

    // Create a sandbox with ttl=0, then drop the runtime before GC runs.
    // On restart it must be treated as already expired (dropped, not
    // restored), and its workspace dir cleaned up.
    let id = {
        let rt = persistent_runtime(&root, &db_path);
        let info = rt
            .create(SandboxSpec {
                ttl_seconds: Some(0),
                metadata: None,
                env: None,
                template: None,
            })
            .await
            .unwrap();
        // Give the ttl=0 expiry a chance to actually elapse.
        tokio::time::sleep(Duration::from_millis(50)).await;
        info.id
    };

    let rt = persistent_runtime(&root, &db_path);
    let listed = rt.list().await.unwrap();
    assert!(
        !listed.iter().any(|s| s.id == id),
        "an already-expired sandbox must not be restored"
    );
}

#[tokio::test]
async fn persistence_removed_sandbox_is_gone_after_restart() {
    let root = temp_dir("persist_removed");
    let db_path = root.join("lightsandbox.redb");

    let (kept, removed) = {
        let rt = persistent_runtime(&root, &db_path);
        let kept = rt.create(SandboxSpec::default()).await.unwrap();
        let removed = rt.create(SandboxSpec::default()).await.unwrap();
        rt.remove(&removed.id).await.unwrap();
        (kept.id, removed.id)
    };

    let rt = persistent_runtime(&root, &db_path);
    let listed = rt.list().await.unwrap();
    assert_eq!(listed.len(), 1, "only the non-removed sandbox survives");
    assert!(listed.iter().any(|s| s.id == kept));
    assert!(!listed.iter().any(|s| s.id == removed));
}

// --- resource-limit enforcement ---

#[tokio::test]
async fn max_sandboxes_limit_is_enforced() {
    // Capacity is a semaphore whose permits are held for each sandbox's
    // lifetime. A create past the limit must surface a RuntimeError rather
    // than block or silently succeed; removing one must release its permit.
    let root = temp_dir("max_sandboxes");
    let limits = ResourceLimits {
        max_sandboxes: 2,
        ..test_limits()
    };
    let rt = runtime_with_limits(&root, limits);

    rt.create(SandboxSpec::default()).await.unwrap();
    rt.create(SandboxSpec::default()).await.unwrap();

    let err = rt.create(SandboxSpec::default()).await.unwrap_err();
    assert!(
        matches!(err, LightSandboxError::RuntimeError(_)),
        "over-capacity create should be a RuntimeError, got {err:?}"
    );

    // Removing one frees a slot, so a new create succeeds again.
    let oldest = rt.list().await.unwrap();
    rt.remove(&oldest[0].id).await.unwrap();
    rt.create(SandboxSpec::default())
        .await
        .expect("create should succeed after a slot was freed");
}

#[tokio::test]
async fn stdout_size_cap_truncates_without_erroring() {
    // `capped_read` keeps the first `max_stdout_bytes` and drains the rest,
    // so an oversized stdout is truncated, not failed — the command still
    // reports a clean exit and the process never blocks on a full pipe.
    let root = temp_dir("stdout_cap");
    let limits = ResourceLimits {
        max_stdout_bytes: 8,
        ..test_limits()
    };
    let rt = runtime_with_limits(&root, limits);
    let info = rt.create(SandboxSpec::default()).await.unwrap();

    let result = rt
        .exec(
            &info.id,
            ExecRequest {
                cmd: echo_cmd("abcdefghijklmnopqrstuvwxyz"),
                timeout_seconds: None,
                env: None,
            },
        )
        .await
        .unwrap();

    assert_eq!(result.exit_code, 0, "echo should still succeed");
    assert!(!result.timed_out);
    assert!(
        result.stdout.len() <= 8,
        "stdout must be capped at 8 bytes, got {}: {:?}",
        result.stdout.len(),
        result.stdout
    );
    assert!(
        !result.stdout.is_empty(),
        "some output should still come through"
    );
}

#[tokio::test]
async fn oversized_read_is_rejected() {
    // Symmetric counterpart to `oversized_file_write_is_rejected`: a file
    // written within the write cap but over the (smaller) read cap must fail
    // with OUTPUT_TOO_LARGE rather than returning a huge body.
    let root = temp_dir("read_oversize");
    let limits = ResourceLimits {
        max_read_file_bytes: 16,
        ..test_limits()
    };
    let rt = runtime_with_limits(&root, limits);
    let info = rt.create(SandboxSpec::default()).await.unwrap();

    // 64 bytes: within the 1024-byte write cap, over the 16-byte read cap.
    rt.write_file(&info.id, "data.bin", vec![0u8; 64])
        .await
        .unwrap();

    let err = rt.read_file(&info.id, "data.bin").await.unwrap_err();
    assert!(
        matches!(err, LightSandboxError::OutputTooLarge),
        "oversized read should be OutputTooLarge, got {err:?}"
    );
}

#[tokio::test]
async fn exec_merges_request_env_over_sandbox_env() {
    // exec clones the sandbox's env, then layers the request's env on top
    // (`merged_env.extend(req.env)`). So sandbox-only vars survive, request
    // vars are added, and on conflict the request value wins.
    let root = temp_dir("env_merge");
    let rt = test_runtime(&root);
    let info = rt
        .create(SandboxSpec {
            ttl_seconds: None,
            metadata: None,
            env: Some(
                [
                    ("BASE".to_string(), "from_sandbox".to_string()),
                    ("SHARED".to_string(), "sandbox_value".to_string()),
                ]
                .into(),
            ),
            template: None,
        })
        .await
        .unwrap();

    #[cfg(windows)]
    let cmd = "echo %BASE% %SHARED% %EXTRA%";
    #[cfg(not(windows))]
    let cmd = "echo $BASE $SHARED $EXTRA";

    let result = rt
        .exec(
            &info.id,
            ExecRequest {
                cmd: cmd.to_string(),
                timeout_seconds: None,
                env: Some(
                    [
                        ("SHARED".to_string(), "request_value".to_string()),
                        ("EXTRA".to_string(), "from_request".to_string()),
                    ]
                    .into(),
                ),
            },
        )
        .await
        .unwrap();

    assert_eq!(result.exit_code, 0);
    let out = result.stdout;
    // BASE: only in the sandbox env -> present.
    assert!(
        out.contains("from_sandbox"),
        "BASE should come from sandbox env: {out:?}"
    );
    // SHARED: in both -> request wins, sandbox value must not leak.
    assert!(
        out.contains("request_value"),
        "SHARED should be overridden by the request: {out:?}"
    );
    assert!(
        !out.contains("sandbox_value"),
        "SHARED's sandbox value must not survive the override: {out:?}"
    );
    // EXTRA: only in the request -> present.
    assert!(
        out.contains("from_request"),
        "EXTRA should come from the request env: {out:?}"
    );
}
