use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A sandbox identifier, formatted as `sbx_<12 hex chars>`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SandboxId(pub String);

impl SandboxId {
    pub fn new() -> Self {
        let hex = Uuid::new_v4().simple().to_string();
        Self(format!("sbx_{}", &hex[..12]))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for SandboxId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for SandboxId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SandboxStatus {
    Creating,
    Running,
    Stopped,
    Failed,
    Expired,
    Removed,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SandboxSpec {
    pub ttl_seconds: Option<u64>,
    pub metadata: Option<HashMap<String, String>>,
    pub env: Option<HashMap<String, String>>,
    /// Name of a template (a subdirectory under the runtime's `templates_dir`)
    /// whose contents are copied into the new workspace at create time. When
    /// `None`, the workspace starts empty.
    pub template: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxInfo {
    pub id: String,
    pub status: SandboxStatus,
    pub workspace_path: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExecRequest {
    pub cmd: String,
    pub timeout_seconds: Option<u64>,
    pub env: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u128,
    pub timed_out: bool,
}

/// One event in a streaming exec session. A session emits zero or more
/// `Stdout`/`Stderr` chunks followed by exactly one terminal event
/// (`Done` on normal completion or timeout, `Error` if the process could
/// not be observed to completion after it had already started).
#[derive(Debug, Clone)]
pub enum ExecOutputEvent {
    Stdout(Vec<u8>),
    Stderr(Vec<u8>),
    Done {
        exit_code: i32,
        timed_out: bool,
        duration_ms: u128,
    },
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileWriteRequest {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileReadResponse {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    pub max_sandboxes: usize,
    pub max_concurrent_exec: usize,
    pub default_ttl_seconds: u64,
    pub default_exec_timeout_seconds: u64,
    pub max_stdout_bytes: usize,
    pub max_stderr_bytes: usize,
    pub max_file_size_bytes: usize,
    pub max_read_file_bytes: usize,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_sandboxes: 100,
            max_concurrent_exec: 20,
            default_ttl_seconds: 600,
            default_exec_timeout_seconds: 60,
            max_stdout_bytes: 1_048_576,
            max_stderr_bytes: 1_048_576,
            max_file_size_bytes: 10_485_760,
            max_read_file_bytes: 10_485_760,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub workspace_root: std::path::PathBuf,
    pub limits: ResourceLimits,
    pub allow_absolute_paths: bool,
    pub allow_path_traversal: bool,
    pub hide_host_paths: bool,
    pub remove_expired: bool,
    /// Root directory of on-disk templates. Each subdirectory is a template
    /// named after the subdirectory; `None` disables template support.
    pub templates_dir: Option<std::path::PathBuf>,
    /// Whether a warm pool of pre-built bare sandboxes is maintained.
    pub pool_enabled: bool,
    /// Target number of idle slots the pool tries to maintain.
    pub pool_min_idle: usize,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            workspace_root: std::path::PathBuf::from("./data/workspaces"),
            limits: ResourceLimits::default(),
            allow_absolute_paths: false,
            allow_path_traversal: false,
            hide_host_paths: true,
            remove_expired: true,
            templates_dir: None,
            pool_enabled: false,
            pool_min_idle: 0,
        }
    }
}
