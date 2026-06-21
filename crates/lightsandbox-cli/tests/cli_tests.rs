use std::io::Write;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde_json::Value;

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn write_config(workspace_root: &Path, port: u16) -> PathBuf {
    let config_path = workspace_root.join("config.toml");
    let contents = format!(
        r#"
[server]
host = "127.0.0.1"
port = {port}

[runtime]
type = "local"
workspace_root = "{workspace_root}"

[limits]
max_sandboxes = 100
max_concurrent_exec = 20
default_ttl_seconds = 600
default_exec_timeout_seconds = 60
max_stdout_bytes = 1048576
max_stderr_bytes = 1048576
max_file_size_bytes = 10485760
max_read_file_bytes = 10485760

[gc]
enabled = false
interval_seconds = 30
remove_expired = true

[security]
allow_absolute_paths = false
allow_path_traversal = false
hide_host_paths = true
"#,
        port = port,
        workspace_root = workspace_root
            .join("data")
            .display()
            .to_string()
            .replace('\\', "/"),
    );
    std::fs::write(&config_path, contents).unwrap();
    config_path
}

/// Starts a real lightsandbox-server on its own thread/runtime and waits
/// until it accepts TCP connections, so CLI subprocess tests exercise the
/// actual HTTP stack rather than a mock.
fn start_server() -> (String, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let port = free_port();
    let config_path = write_config(dir.path(), port);

    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _ = rt.block_on(lightsandbox_server::run(&config_path));
    });

    let addr = format!("127.0.0.1:{port}");
    for _ in 0..100 {
        if TcpStream::connect(&addr).is_ok() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    (format!("http://{addr}"), dir)
}

/// Finds a real python.exe on PATH, skipping the Windows "App Execution
/// Alias" stub that resolves under a Git-Bash-derived PATH but produces no
/// output (see lightsandbox-runtime-local's integration tests for the same
/// issue).
#[cfg(windows)]
fn find_python() -> Option<String> {
    let path = std::env::var("PATH").ok()?;
    for dir in path.split(';') {
        if dir.to_lowercase().contains("windowsapps") {
            continue;
        }
        let candidate = Path::new(dir).join("python.exe");
        if candidate.is_file() {
            return Some(candidate.to_string_lossy().into_owned());
        }
    }
    None
}

#[cfg(not(windows))]
fn find_python() -> Option<String> {
    Some("python3".to_string())
}

fn cli(base_url: &str, args: &[&str]) -> (i32, Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_lightsandbox"))
        .arg("--base-url")
        .arg(base_url)
        .arg("--json")
        .args(args)
        .output()
        .expect("failed to run lightsandbox CLI binary");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let value = if stdout.trim().is_empty() {
        Value::Null
    } else {
        serde_json::from_str(stdout.trim()).unwrap_or(Value::Null)
    };
    (output.status.code().unwrap_or(-1), value)
}

/// Like `cli`, but returns the raw stdout bytes without JSON parsing — for
/// `download` (to stdout), whose output is an opaque octet-stream.
fn cli_raw(base_url: &str, args: &[&str]) -> (i32, Vec<u8>) {
    let output = Command::new(env!("CARGO_BIN_EXE_lightsandbox"))
        .arg("--base-url")
        .arg(base_url)
        .args(args)
        .output()
        .expect("failed to run lightsandbox CLI binary");
    (output.status.code().unwrap_or(-1), output.stdout)
}

#[test]
fn create_list_round_trip() {
    let (base_url, _dir) = start_server();

    let (code, created) = cli(&base_url, &["create", "--ttl-seconds", "120"]);
    assert_eq!(code, 0);
    let id = created["id"].as_str().unwrap().to_string();
    assert!(id.starts_with("sbx_"));

    let (code, listed) = cli(&base_url, &["list"]);
    assert_eq!(code, 0);
    let ids: Vec<&str> = listed
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&id.as_str()));
}

#[test]
fn write_exec_read_remove_round_trip() {
    let (base_url, _dir) = start_server();

    let (_, created) = cli(&base_url, &["create", "--ttl-seconds", "120"]);
    let id = created["id"].as_str().unwrap().to_string();

    let mut local_file = tempfile::NamedTempFile::new().unwrap();
    write!(local_file, "print('hi from cli test')").unwrap();
    let local_path = local_file.path().to_str().unwrap();

    let (code, written) = cli(&base_url, &["write", &id, local_path, "main.py"]);
    assert_eq!(code, 0);
    assert_eq!(written["written"], true);

    let python = find_python().expect("no usable python.exe found on PATH");
    let exec_cmd = format!("{python} main.py");
    let (code, exec_result) = cli(&base_url, &["exec", &id, &exec_cmd]);
    assert_eq!(code, 0);
    assert_eq!(exec_result["exit_code"], 0);
    assert!(exec_result["stdout"]
        .as_str()
        .unwrap()
        .contains("hi from cli test"));

    let (code, read_result) = cli(&base_url, &["read", &id, "main.py"]);
    assert_eq!(code, 0);
    assert!(read_result["content"]
        .as_str()
        .unwrap()
        .contains("hi from cli test"));

    let (code, removed) = cli(&base_url, &["rm", &id]);
    assert_eq!(code, 0);
    assert_eq!(removed["removed"], true);

    let (code, _) = cli(&base_url, &["exec", &id, "echo gone"]);
    assert_ne!(code, 0);
}

#[test]
fn read_with_special_characters_in_path_round_trips() {
    let (base_url, _dir) = start_server();

    let (_, created) = cli(&base_url, &["create", "--ttl-seconds", "120"]);
    let id = created["id"].as_str().unwrap().to_string();

    let mut local_file = tempfile::NamedTempFile::new().unwrap();
    write!(local_file, "content with special path").unwrap();
    let local_path = local_file.path().to_str().unwrap();

    // The remote filename contains characters that must be percent-encoded
    // in the GET /files?path= query string (space, &, #) for `read` to find
    // the right file rather than silently truncating/misparsing the query.
    let remote_path = "a file&name#1.txt";

    let (code, written) = cli(&base_url, &["write", &id, local_path, remote_path]);
    assert_eq!(code, 0);
    assert_eq!(written["written"], true);

    let (code, read_result) = cli(&base_url, &["read", &id, remote_path]);
    assert_eq!(code, 0);
    assert!(read_result["content"]
        .as_str()
        .unwrap()
        .contains("content with special path"));
}

#[test]
fn exec_on_unknown_sandbox_fails_with_nonzero_exit() {
    let (base_url, _dir) = start_server();
    let (code, _) = cli(&base_url, &["exec", "sbx_doesnotexist", "echo hi"]);
    assert_ne!(code, 0);
}

#[test]
fn upload_download_round_trip_is_binary_safe() {
    // `upload`/`download` must move a file with non-UTF-8 bytes losslessly —
    // the JSON-text `write`/`read` commands cannot represent these bytes, so
    // this is the exact gap the binary commands exist to fill.
    let (base_url, _dir) = start_server();
    let (_, created) = cli(&base_url, &["create", "--ttl-seconds", "120"]);
    let id = created["id"].as_str().unwrap().to_string();

    // NUL, 0xFF, 0xFE, stray continuation bytes — all invalid as UTF-8.
    let payload = [0x00u8, 0xFF, 0xFE, 0x80, 0x01, 0x02, 0xC3, 0x28, 0xFF, 0x00];
    let mut local = tempfile::NamedTempFile::new().unwrap();
    std::io::Write::write_all(&mut local, &payload).unwrap();
    let local_path = local.path().to_str().unwrap().to_string();

    let (code, _) = cli(&base_url, &["upload", &id, &local_path, "blob.bin"]);
    assert_eq!(code, 0, "upload should succeed");

    // File-to-file download: bytes must match exactly.
    let downloaded = tempfile::NamedTempFile::new().unwrap();
    let downloaded_path = downloaded.path().to_str().unwrap().to_string();
    // NamedTempFile creates the file; remove it so the CLI writes a fresh one
    // (otherwise we'd be comparing against its initial empty contents).
    std::fs::remove_file(&downloaded_path).unwrap();
    let (code, _) = cli(&base_url, &["download", &id, "blob.bin", &downloaded_path]);
    assert_eq!(code, 0, "download to file should succeed");
    let got = std::fs::read(&downloaded_path).unwrap();
    assert_eq!(got, payload, "downloaded file must be byte-identical");

    // Download-to-stdout: raw bytes, no JSON wrapping.
    let (code, stdout_bytes) = cli_raw(&base_url, &["download", &id, "blob.bin"]);
    assert_eq!(code, 0);
    assert_eq!(
        stdout_bytes, payload,
        "download to stdout must be byte-identical"
    );

    let (code, _) = cli(&base_url, &["rm", &id]);
    assert_eq!(code, 0);
}
