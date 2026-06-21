use std::time::Duration;

use lightsandbox_core::{ExecRequest, ResourceLimits, RuntimeConfig, SandboxRuntime, SandboxSpec};
use lightsandbox_runtime_local::LocalProcessRuntime;

fn test_runtime(root: &std::path::Path) -> LocalProcessRuntime {
    let config = RuntimeConfig {
        workspace_root: root.to_path_buf(),
        limits: ResourceLimits {
            max_sandboxes: 1000,
            max_concurrent_exec: 50,
            default_ttl_seconds: 600,
            default_exec_timeout_seconds: 5,
            max_stdout_bytes: 4096,
            max_stderr_bytes: 4096,
            max_file_size_bytes: 1024,
            max_read_file_bytes: 1024,
        },
        allow_absolute_paths: false,
        allow_path_traversal: false,
        hide_host_paths: true,
    };
    LocalProcessRuntime::new(config)
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
    let cmd = "python3 -c print(1+1)".to_string();

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
