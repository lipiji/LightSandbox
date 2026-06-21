use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use chrono::{Duration as ChronoDuration, Utc};
use dashmap::DashMap;
use lightsandbox_core::{
    ExecRequest, ExecResult, LightSandboxError, RuntimeConfig, SandboxInfo, SandboxRuntime,
    SandboxSpec, SandboxStatus,
};
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::paths::safe_path;

struct SandboxEntry {
    info: SandboxInfo,
    env: HashMap<String, String>,
    // Held for the lifetime of the sandbox; dropping it (on remove/expiry
    // cleanup) returns the slot to `sandbox_semaphore`.
    _permit: OwnedSemaphorePermit,
}

pub struct LocalProcessRuntime {
    config: RuntimeConfig,
    sandboxes: DashMap<String, SandboxEntry>,
    exec_semaphore: Arc<Semaphore>,
    sandbox_semaphore: Arc<Semaphore>,
}

impl LocalProcessRuntime {
    pub fn new(config: RuntimeConfig) -> Self {
        let exec_semaphore = Arc::new(Semaphore::new(config.limits.max_concurrent_exec.max(1)));
        let sandbox_semaphore = Arc::new(Semaphore::new(config.limits.max_sandboxes.max(1)));
        Self {
            config,
            sandboxes: DashMap::new(),
            exec_semaphore,
            sandbox_semaphore,
        }
    }

    fn workspace_dir(&self, id: &str) -> PathBuf {
        self.config.workspace_root.join(id)
    }

    fn get_active(
        &self,
        id: &str,
    ) -> Result<dashmap::mapref::one::Ref<'_, String, SandboxEntry>, LightSandboxError> {
        let entry = self
            .sandboxes
            .get(id)
            .ok_or(LightSandboxError::SandboxNotFound)?;
        if let Some(expires_at) = entry.info.expires_at {
            if expires_at <= Utc::now() {
                return Err(LightSandboxError::SandboxExpired);
            }
        }
        Ok(entry)
    }

    /// Logs the full I/O error detail server-side, but only includes it in
    /// the client-facing message when `hide_host_paths` is disabled — keeps
    /// host filesystem details out of API responses by default.
    fn io_error(&self, context: &str, e: std::io::Error) -> LightSandboxError {
        tracing::error!(error = %e, context, "io operation failed");
        if self.config.hide_host_paths {
            LightSandboxError::RuntimeError(context.to_string())
        } else {
            LightSandboxError::RuntimeError(format!("{context}: {e}"))
        }
    }
}

fn shell_command(cmd: &str) -> Command {
    #[cfg(windows)]
    {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(cmd);
        c
    }
    #[cfg(not(windows))]
    {
        let mut c = Command::new("sh");
        c.arg("-c").arg(cmd);
        // New process group (pgid = child's pid) so the whole tree spawned
        // by the shell can be killed at once on timeout, not just `sh`.
        c.process_group(0);
        c
    }
}

/// Reads `reader` to EOF, retaining at most `max_bytes` but still draining
/// everything past that cap so the child never blocks on a full pipe.
async fn capped_read<R>(mut reader: R, max_bytes: usize) -> Vec<u8>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut buf = [0u8; 8192];
    let mut out = Vec::with_capacity(max_bytes.min(64 * 1024));
    loop {
        match reader.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => {
                if out.len() < max_bytes {
                    let take = (max_bytes - out.len()).min(n);
                    out.extend_from_slice(&buf[..take]);
                }
            }
            Err(_) => break,
        }
    }
    out
}

#[cfg(unix)]
async fn kill_process_tree(pid: Option<u32>) {
    if let Some(pid) = pid {
        // SAFETY: signaling a process group by pid is a plain libc call with
        // no preconditions beyond a valid pid, which we have from `Child::id`.
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
    }
}

#[cfg(windows)]
async fn kill_process_tree(pid: Option<u32>) {
    if let Some(pid) = pid {
        let _ = tokio::process::Command::new("taskkill")
            .args(["/T", "/F", "/PID", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;
    }
}

#[async_trait]
impl SandboxRuntime for LocalProcessRuntime {
    async fn create(&self, spec: SandboxSpec) -> Result<SandboxInfo, LightSandboxError> {
        let permit = self
            .sandbox_semaphore
            .clone()
            .try_acquire_owned()
            .map_err(|_| LightSandboxError::RuntimeError("max_sandboxes limit reached".into()))?;

        let id = lightsandbox_core::SandboxId::new();
        let workspace_path = self.workspace_dir(id.as_str());
        tokio::fs::create_dir_all(&workspace_path)
            .await
            .map_err(|e| self.io_error("creating workspace", e))?;

        let created_at = Utc::now();
        let ttl_seconds = spec
            .ttl_seconds
            .unwrap_or(self.config.limits.default_ttl_seconds);
        let expires_at = Some(created_at + ChronoDuration::seconds(ttl_seconds as i64));

        let info = SandboxInfo {
            id: id.to_string(),
            status: SandboxStatus::Running,
            workspace_path: "/workspace".to_string(),
            created_at,
            expires_at,
            metadata: spec.metadata.unwrap_or_default(),
        };

        self.sandboxes.insert(
            id.to_string(),
            SandboxEntry {
                info: info.clone(),
                env: spec.env.unwrap_or_default(),
                _permit: permit,
            },
        );

        Ok(info)
    }

    async fn list(&self) -> Result<Vec<SandboxInfo>, LightSandboxError> {
        let mut infos: Vec<SandboxInfo> = self.sandboxes.iter().map(|e| e.info.clone()).collect();
        infos.sort_by_key(|i| i.created_at);
        Ok(infos)
    }

    async fn get(&self, id: &str) -> Result<SandboxInfo, LightSandboxError> {
        self.sandboxes
            .get(id)
            .map(|e| e.info.clone())
            .ok_or(LightSandboxError::SandboxNotFound)
    }

    async fn exec(&self, id: &str, req: ExecRequest) -> Result<ExecResult, LightSandboxError> {
        let (workspace_path, mut merged_env) = {
            let entry = self.get_active(id)?;
            (self.workspace_dir(&entry.info.id), entry.env.clone())
        };
        if let Some(env) = req.env {
            merged_env.extend(env);
        }

        let _permit = self
            .exec_semaphore
            .acquire()
            .await
            .map_err(|e| LightSandboxError::RuntimeError(e.to_string()))?;

        let timeout_secs = req
            .timeout_seconds
            .unwrap_or(self.config.limits.default_exec_timeout_seconds);
        let timeout_dur = std::time::Duration::from_secs(timeout_secs);

        let mut command = shell_command(&req.cmd);
        command.current_dir(&workspace_path);
        command.envs(merged_env);
        command.kill_on_drop(true);
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());

        let start = Instant::now();
        let mut child = command
            .spawn()
            .map_err(|e| LightSandboxError::ExecFailed(e.to_string()))?;

        let pid = child.id();
        let stdout = child.stdout.take().expect("stdout was piped");
        let stderr = child.stderr.take().expect("stderr was piped");
        let max_stdout = self.config.limits.max_stdout_bytes;
        let max_stderr = self.config.limits.max_stderr_bytes;

        let output_fut = async {
            tokio::join!(
                capped_read(stdout, max_stdout),
                capped_read(stderr, max_stderr),
                child.wait(),
            )
        };

        match tokio::time::timeout(timeout_dur, output_fut).await {
            Ok((stdout_bytes, stderr_bytes, Ok(status))) => {
                let duration_ms = start.elapsed().as_millis();
                Ok(ExecResult {
                    exit_code: status.code().unwrap_or(-1),
                    stdout: String::from_utf8_lossy(&stdout_bytes).into_owned(),
                    stderr: String::from_utf8_lossy(&stderr_bytes).into_owned(),
                    duration_ms,
                    timed_out: false,
                })
            }
            Ok((_, _, Err(e))) => Err(LightSandboxError::ExecFailed(e.to_string())),
            Err(_elapsed) => {
                kill_process_tree(pid).await;
                Ok(ExecResult {
                    exit_code: -1,
                    stdout: String::new(),
                    stderr: String::new(),
                    duration_ms: start.elapsed().as_millis(),
                    timed_out: true,
                })
            }
        }
    }

    async fn write_file(
        &self,
        id: &str,
        path: &str,
        content: Vec<u8>,
    ) -> Result<(), LightSandboxError> {
        let workspace_path = {
            let entry = self.get_active(id)?;
            self.workspace_dir(&entry.info.id)
        };

        if content.len() > self.config.limits.max_file_size_bytes {
            return Err(LightSandboxError::FileTooLarge);
        }

        let target = safe_path(
            &workspace_path,
            path,
            self.config.allow_absolute_paths,
            self.config.allow_path_traversal,
        )?;

        if let Some(parent) = target.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| self.io_error("creating parent directory", e))?;
        }
        tokio::fs::write(&target, content)
            .await
            .map_err(|e| self.io_error("writing file", e))?;
        Ok(())
    }

    async fn read_file(&self, id: &str, path: &str) -> Result<Vec<u8>, LightSandboxError> {
        let workspace_path = {
            let entry = self.get_active(id)?;
            self.workspace_dir(&entry.info.id)
        };

        let target = safe_path(
            &workspace_path,
            path,
            self.config.allow_absolute_paths,
            self.config.allow_path_traversal,
        )?;

        let metadata = tokio::fs::metadata(&target)
            .await
            .map_err(|_| LightSandboxError::InvalidPath(format!("file not found: {path}")))?;
        if metadata.len() as usize > self.config.limits.max_read_file_bytes {
            return Err(LightSandboxError::OutputTooLarge);
        }

        tokio::fs::read(&target)
            .await
            .map_err(|e| self.io_error("reading file", e))
    }

    async fn remove(&self, id: &str) -> Result<(), LightSandboxError> {
        if !self.sandboxes.contains_key(id) {
            return Err(LightSandboxError::SandboxNotFound);
        }
        let workspace_path = self.workspace_dir(id);
        if workspace_path.exists() {
            tokio::fs::remove_dir_all(&workspace_path)
                .await
                .map_err(|e| self.io_error("removing workspace", e))?;
        }
        self.sandboxes.remove(id);
        Ok(())
    }

    async fn cleanup_expired(&self) -> Result<usize, LightSandboxError> {
        let now = Utc::now();
        let expired_ids: Vec<String> = self
            .sandboxes
            .iter()
            .filter(|e| {
                e.info.status != SandboxStatus::Expired
                    && e.info.expires_at.map(|exp| exp <= now).unwrap_or(false)
            })
            .map(|e| e.key().clone())
            .collect();

        let mut processed = 0;
        for id in expired_ids {
            if self.config.remove_expired {
                let workspace_path = self.workspace_dir(&id);
                if workspace_path.exists() {
                    let _ = tokio::fs::remove_dir_all(&workspace_path).await;
                }
                if self.sandboxes.remove(&id).is_some() {
                    processed += 1;
                }
            } else if let Some(mut entry) = self.sandboxes.get_mut(&id) {
                entry.info.status = SandboxStatus::Expired;
                processed += 1;
            }
        }
        Ok(processed)
    }
}
