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
use tokio::process::Command;
use tokio::sync::Semaphore;

use crate::paths::safe_path;

struct SandboxEntry {
    info: SandboxInfo,
    env: HashMap<String, String>,
}

pub struct LocalProcessRuntime {
    config: RuntimeConfig,
    sandboxes: DashMap<String, SandboxEntry>,
    exec_semaphore: Arc<Semaphore>,
}

impl LocalProcessRuntime {
    pub fn new(config: RuntimeConfig) -> Self {
        let exec_semaphore = Arc::new(Semaphore::new(config.limits.max_concurrent_exec.max(1)));
        Self {
            config,
            sandboxes: DashMap::new(),
            exec_semaphore,
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
        c
    }
}

fn truncate_output(bytes: Vec<u8>, max_bytes: usize) -> String {
    if bytes.len() > max_bytes {
        String::from_utf8_lossy(&bytes[..max_bytes]).into_owned()
    } else {
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

#[async_trait]
impl SandboxRuntime for LocalProcessRuntime {
    async fn create(&self, spec: SandboxSpec) -> Result<SandboxInfo, LightSandboxError> {
        if self.sandboxes.len() >= self.config.limits.max_sandboxes {
            return Err(LightSandboxError::RuntimeError(
                "max_sandboxes limit reached".into(),
            ));
        }

        let id = lightsandbox_core::SandboxId::new();
        let workspace_path = self.workspace_dir(id.as_str());
        tokio::fs::create_dir_all(&workspace_path)
            .await
            .map_err(|e| LightSandboxError::RuntimeError(e.to_string()))?;

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
        let child = command
            .spawn()
            .map_err(|e| LightSandboxError::ExecFailed(e.to_string()))?;

        match tokio::time::timeout(timeout_dur, child.wait_with_output()).await {
            Ok(Ok(output)) => {
                let duration_ms = start.elapsed().as_millis();
                let stdout = truncate_output(output.stdout, self.config.limits.max_stdout_bytes);
                let stderr = truncate_output(output.stderr, self.config.limits.max_stderr_bytes);
                Ok(ExecResult {
                    exit_code: output.status.code().unwrap_or(-1),
                    stdout,
                    stderr,
                    duration_ms,
                    timed_out: false,
                })
            }
            Ok(Err(e)) => Err(LightSandboxError::ExecFailed(e.to_string())),
            Err(_elapsed) => Ok(ExecResult {
                exit_code: -1,
                stdout: String::new(),
                stderr: String::new(),
                duration_ms: start.elapsed().as_millis(),
                timed_out: true,
            }),
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

        let target = safe_path(&workspace_path, path, self.config.allow_absolute_paths)?;

        if let Some(parent) = target.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| LightSandboxError::RuntimeError(e.to_string()))?;
        }
        tokio::fs::write(&target, content)
            .await
            .map_err(|e| LightSandboxError::RuntimeError(e.to_string()))?;
        Ok(())
    }

    async fn read_file(&self, id: &str, path: &str) -> Result<Vec<u8>, LightSandboxError> {
        let workspace_path = {
            let entry = self.get_active(id)?;
            self.workspace_dir(&entry.info.id)
        };

        let target = safe_path(&workspace_path, path, self.config.allow_absolute_paths)?;

        let metadata = tokio::fs::metadata(&target)
            .await
            .map_err(|_| LightSandboxError::InvalidPath(format!("file not found: {path}")))?;
        if metadata.len() as usize > self.config.limits.max_read_file_bytes {
            return Err(LightSandboxError::OutputTooLarge);
        }

        tokio::fs::read(&target)
            .await
            .map_err(|e| LightSandboxError::RuntimeError(e.to_string()))
    }

    async fn remove(&self, id: &str) -> Result<(), LightSandboxError> {
        if !self.sandboxes.contains_key(id) {
            return Err(LightSandboxError::SandboxNotFound);
        }
        let workspace_path = self.workspace_dir(id);
        if workspace_path.exists() {
            tokio::fs::remove_dir_all(&workspace_path)
                .await
                .map_err(|e| LightSandboxError::RuntimeError(e.to_string()))?;
        }
        self.sandboxes.remove(id);
        Ok(())
    }

    async fn cleanup_expired(&self) -> Result<usize, LightSandboxError> {
        let now = Utc::now();
        let expired_ids: Vec<String> = self
            .sandboxes
            .iter()
            .filter(|e| e.info.expires_at.map(|exp| exp <= now).unwrap_or(false))
            .map(|e| e.key().clone())
            .collect();

        let mut removed = 0;
        for id in expired_ids {
            let workspace_path = self.workspace_dir(&id);
            if workspace_path.exists() {
                let _ = tokio::fs::remove_dir_all(&workspace_path).await;
            }
            if self.sandboxes.remove(&id).is_some() {
                removed += 1;
            }
        }
        Ok(removed)
    }
}
