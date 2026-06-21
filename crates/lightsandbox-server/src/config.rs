use std::path::{Path, PathBuf};

use lightsandbox_core::{LightSandboxError, ResourceLimits, RuntimeConfig};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct ServerSection {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RuntimeSection {
    #[serde(rename = "type")]
    pub kind: String,
    pub workspace_root: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LimitsSection {
    pub max_sandboxes: usize,
    pub max_concurrent_exec: usize,
    pub default_ttl_seconds: u64,
    pub default_exec_timeout_seconds: u64,
    pub max_stdout_bytes: usize,
    pub max_stderr_bytes: usize,
    pub max_file_size_bytes: usize,
    pub max_read_file_bytes: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GcSection {
    pub enabled: bool,
    pub interval_seconds: u64,
    pub remove_expired: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SecuritySection {
    pub allow_absolute_paths: bool,
    pub allow_path_traversal: bool,
    pub hide_host_paths: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct TemplatesSection {
    /// Root directory of on-disk templates; absence disables templates.
    pub dir: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PoolSection {
    pub enabled: bool,
    pub min_idle: usize,
}

/// Opt-in sandbox-metadata persistence across restarts. Off by default so
/// v0.1's zero-database guarantee still holds unless an operator sets
/// `enabled = true`. When enabled, sandbox metadata is mirrored to a `redb`
/// file at `path` and restored on the next startup.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PersistenceSection {
    #[serde(default)]
    pub enabled: bool,
    /// Path to the `redb` database file. Only consulted when `enabled = true`.
    #[serde(default = "default_persistence_path")]
    pub path: String,
}

fn default_persistence_path() -> String {
    "data/lightsandbox.redb".to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub server: ServerSection,
    pub runtime: RuntimeSection,
    pub limits: LimitsSection,
    pub gc: GcSection,
    pub security: SecuritySection,
    #[serde(default)]
    pub templates: TemplatesSection,
    #[serde(default)]
    pub pool: PoolSection,
    #[serde(default)]
    pub persistence: PersistenceSection,
}

impl AppConfig {
    pub fn load(path: &Path) -> Result<Self, LightSandboxError> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| LightSandboxError::ConfigError(format!("reading config: {e}")))?;
        toml::from_str(&text)
            .map_err(|e| LightSandboxError::ConfigError(format!("parsing config: {e}")))
    }

    pub fn socket_addr(&self) -> Result<std::net::SocketAddr, LightSandboxError> {
        format!("{}:{}", self.server.host, self.server.port)
            .parse()
            .map_err(|e| LightSandboxError::ConfigError(format!("invalid server address: {e}")))
    }

    pub fn runtime_config(&self) -> RuntimeConfig {
        RuntimeConfig {
            workspace_root: PathBuf::from(&self.runtime.workspace_root),
            limits: ResourceLimits {
                max_sandboxes: self.limits.max_sandboxes,
                max_concurrent_exec: self.limits.max_concurrent_exec,
                default_ttl_seconds: self.limits.default_ttl_seconds,
                default_exec_timeout_seconds: self.limits.default_exec_timeout_seconds,
                max_stdout_bytes: self.limits.max_stdout_bytes,
                max_stderr_bytes: self.limits.max_stderr_bytes,
                max_file_size_bytes: self.limits.max_file_size_bytes,
                max_read_file_bytes: self.limits.max_read_file_bytes,
            },
            allow_absolute_paths: self.security.allow_absolute_paths,
            allow_path_traversal: self.security.allow_path_traversal,
            hide_host_paths: self.security.hide_host_paths,
            remove_expired: self.gc.remove_expired,
            templates_dir: self.templates.dir.as_ref().map(PathBuf::from),
            pool_enabled: self.pool.enabled,
            pool_min_idle: self.pool.min_idle,
            persistence_db_path: if self.persistence.enabled {
                Some(PathBuf::from(&self.persistence.path))
            } else {
                None
            },
        }
    }
}
