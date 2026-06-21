use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use async_trait::async_trait;
use chrono::{Duration as ChronoDuration, Utc};
use dashmap::DashMap;
use lightsandbox_core::{
    ExecOutputEvent, ExecRequest, ExecResult, LightSandboxError, MetricsSnapshot, RuntimeConfig,
    SandboxId, SandboxInfo, SandboxRuntime, SandboxSpec, SandboxStatus,
};
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::sync::mpsc::Sender;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::metrics_collector::MetricsCollector;
use crate::paths::safe_path;

struct SandboxEntry {
    info: SandboxInfo,
    env: HashMap<String, String>,
    // Held for the lifetime of the sandbox; dropping it (on remove/expiry
    // cleanup) returns the slot to `sandbox_semaphore`.
    _permit: OwnedSemaphorePermit,
}

/// A pre-built, unassigned sandbox kept in the warm pool: its workspace dir
/// already exists and its `max_sandboxes` permit is already held. Lives outside
/// the `sandboxes` DashMap so it is invisible to `list()` and exempt from
/// TTL/GC until handed out by `create`.
struct IdleSlot {
    id: SandboxId,
    workspace_path: PathBuf,
    permit: OwnedSemaphorePermit,
}

pub struct LocalProcessRuntime {
    config: RuntimeConfig,
    sandboxes: DashMap<String, SandboxEntry>,
    exec_semaphore: Arc<Semaphore>,
    sandbox_semaphore: Arc<Semaphore>,
    metrics: Arc<MetricsCollector>,
    idle_slots: Arc<Mutex<Vec<IdleSlot>>>,
    /// Number of replenish tasks currently in flight, used to bound spawns.
    replenish_in_flight: Arc<AtomicUsize>,
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
            metrics: Arc::new(MetricsCollector::new()),
            idle_slots: Arc::new(Mutex::new(Vec::new())),
            replenish_in_flight: Arc::new(AtomicUsize::new(0)),
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

    // --- warm pool helpers ---

    /// Pops a pre-warmed idle slot, if one is available.
    fn pop_idle_slot(&self) -> Option<IdleSlot> {
        self.idle_slots
            .lock()
            .ok()
            .and_then(|mut slots| slots.pop())
    }

    /// Builds a brand-new slot: acquires a `max_sandboxes` permit, mints an id,
    /// and creates the workspace dir. Returns `None` if the permit cannot be
    /// acquired (capacity saturated) or the dir cannot be created.
    async fn fresh_slot(
        &self,
    ) -> Result<(SandboxId, PathBuf, OwnedSemaphorePermit), LightSandboxError> {
        let permit = self
            .sandbox_semaphore
            .clone()
            .try_acquire_owned()
            .map_err(|_| LightSandboxError::RuntimeError("max_sandboxes limit reached".into()))?;
        let id = SandboxId::new();
        let workspace_path = self.workspace_dir(id.as_str());
        tokio::fs::create_dir_all(&workspace_path)
            .await
            .map_err(|e| self.io_error("creating workspace", e))?;
        Ok((id, workspace_path, permit))
    }

    /// Fills the pool up to `pool_min_idle` at startup. Stops early if
    /// `max_sandboxes` capacity is saturated (best-effort).
    pub async fn prewarm(&self) {
        if !self.config.pool_enabled || self.config.pool_min_idle == 0 {
            return;
        }
        let mut built = 0usize;
        for _ in 0..self.config.pool_min_idle {
            if let Some(slot) =
                build_slot(&self.sandbox_semaphore, &self.config.workspace_root).await
            {
                if let Ok(mut slots) = self.idle_slots.lock() {
                    slots.push(slot);
                    built += 1;
                }
            } else {
                break;
            }
        }
        tracing::info!(
            target = self.config.pool_min_idle,
            built,
            "warm pool prewarmed"
        );
    }

    /// Lazily refills the pool after a create consumed a slot. Spawns a
    /// background task only when `idle + in_flight < min_idle`, and uses
    /// `replenish_in_flight` to prevent spawn storms. Fire-and-forget.
    fn maybe_replenish(&self) {
        if self.config.pool_min_idle == 0 {
            return;
        }
        let idle = self.idle_slots.lock().map(|s| s.len()).unwrap_or(0);
        let in_flight = self.replenish_in_flight.load(Ordering::Relaxed);
        if idle + in_flight >= self.config.pool_min_idle {
            return;
        }
        self.replenish_in_flight.fetch_add(1, Ordering::Relaxed);
        let semaphore = Arc::clone(&self.sandbox_semaphore);
        let idle_slots = Arc::clone(&self.idle_slots);
        let in_flight = Arc::clone(&self.replenish_in_flight);
        let workspace_root = self.config.workspace_root.clone();
        tokio::spawn(async move {
            if let Some(slot) = build_slot(&semaphore, &workspace_root).await {
                if let Ok(mut slots) = idle_slots.lock() {
                    slots.push(slot);
                }
            }
            in_flight.fetch_sub(1, Ordering::Relaxed);
        });
    }
}

/// Acquires a permit, mints an id, and creates the workspace dir — the shared
/// building block for both synchronous `prewarm` and the spawned replenish
/// task. Returns `None` on capacity saturation or IO failure.
async fn build_slot(semaphore: &Arc<Semaphore>, workspace_root: &Path) -> Option<IdleSlot> {
    let permit = semaphore.clone().try_acquire_owned().ok()?;
    let id = SandboxId::new();
    let workspace_path = workspace_root.join(id.as_str());
    tokio::fs::create_dir_all(&workspace_path).await.ok()?;
    Some(IdleSlot {
        id,
        workspace_path,
        permit,
    })
}

/// Recursively copies `src` into `dst`, creating `dst`. Async and zero-dep.
/// Symlinks and other special file types are skipped. The recursive call is
/// boxed to give the future a bounded size.
async fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    tokio::fs::create_dir_all(dst).await?;
    let mut entries = tokio::fs::read_dir(src).await?;
    while let Some(entry) = entries.next_entry().await? {
        let from = entry.path();
        let to = dst.join(entry.file_name());
        let file_type = entry.file_type().await?;
        if file_type.is_dir() {
            Box::pin(copy_dir_recursive(&from, &to)).await?;
        } else if file_type.is_file() {
            tokio::fs::copy(&from, &to).await?;
        }
    }
    Ok(())
}

/// Builds the `Command` shared by `exec` and `exec_stream`: shell wrapper,
/// workspace cwd, merged env, and piped stdout/stderr.
fn build_command(workspace_path: &Path, env: &HashMap<String, String>, cmd: &str) -> Command {
    let mut command = shell_command(cmd);
    command.current_dir(workspace_path);
    command.envs(env.clone());
    command.kill_on_drop(true);
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    command
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

/// Reads `reader` to EOF, sending each chunk through `tx` as it arrives
/// instead of buffering. Stops early if the receiver is gone (client
/// disconnected); the caller's overall exec timeout remains the backstop
/// that prevents the child from blocking forever on an undrained pipe.
async fn pump_reader<R>(mut reader: R, tx: Sender<ExecOutputEvent>, wrap: fn(Vec<u8>) -> ExecOutputEvent)
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut buf = [0u8; 8192];
    loop {
        match reader.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if tx.send(wrap(buf[..n].to_vec())).await.is_err() {
                    break;
                }
            }
        }
    }
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
        // Resolve the template source up front so we fail fast (and before
        // touching the pool or filesystem).
        let template_src = match &spec.template {
            Some(name) => {
                let root = self.config.templates_dir.as_ref().ok_or_else(|| {
                    LightSandboxError::InvalidPath("templates not configured".into())
                })?;
                let src = root.join(name);
                if !src.is_dir() {
                    return Err(LightSandboxError::InvalidPath(format!(
                        "template not found: {name}"
                    )));
                }
                Some(src)
            }
            None => None,
        };

        // Obtain a workspace: reuse a pooled bare slot when the create is
        // template-less and the pool is on; otherwise build fresh. A templated
        // create never consumes the pool (slots are bare by design).
        let (id, workspace_path, permit) = if template_src.is_none() && self.config.pool_enabled {
            match self.pop_idle_slot() {
                Some(slot) => (slot.id, slot.workspace_path, slot.permit),
                None => self.fresh_slot().await?,
            }
        } else {
            self.fresh_slot().await?
        };

        // Apply the template, if any, into the now-existing workspace.
        if let Some(src) = template_src {
            copy_dir_recursive(&src, &workspace_path)
                .await
                .map_err(|e| self.io_error("copying template", e))?;
        }

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

        self.metrics.record_create();

        // Lazily refill the pool if this create drained it.
        if self.config.pool_enabled {
            self.maybe_replenish();
        }

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

        let mut command = build_command(&workspace_path, &merged_env, &req.cmd);

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
                self.metrics
                    .record_exec(duration_ms.try_into().unwrap_or(u64::MAX), false);
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
                let duration_ms = start.elapsed().as_millis();
                self.metrics
                    .record_exec(duration_ms.try_into().unwrap_or(u64::MAX), true);
                Ok(ExecResult {
                    exit_code: -1,
                    stdout: String::new(),
                    stderr: String::new(),
                    duration_ms,
                    timed_out: true,
                })
            }
        }
    }

    async fn exec_stream(
        &self,
        id: &str,
        req: ExecRequest,
        tx: Sender<ExecOutputEvent>,
    ) -> Result<(), LightSandboxError> {
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

        let mut command = build_command(&workspace_path, &merged_env, &req.cmd);

        let start = Instant::now();
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(e) => {
                let _ = tx.send(ExecOutputEvent::Error(e.to_string())).await;
                return Ok(());
            }
        };

        let pid = child.id();
        let stdout = child.stdout.take().expect("stdout was piped");
        let stderr = child.stderr.take().expect("stderr was piped");

        let pump_fut = async {
            tokio::join!(
                pump_reader(stdout, tx.clone(), ExecOutputEvent::Stdout),
                pump_reader(stderr, tx.clone(), ExecOutputEvent::Stderr),
                child.wait(),
            )
        };

        match tokio::time::timeout(timeout_dur, pump_fut).await {
            Ok((_, _, Ok(status))) => {
                let duration_ms = start.elapsed().as_millis();
                self.metrics
                    .record_exec(duration_ms.try_into().unwrap_or(u64::MAX), false);
                let _ = tx
                    .send(ExecOutputEvent::Done {
                        exit_code: status.code().unwrap_or(-1),
                        timed_out: false,
                        duration_ms,
                    })
                    .await;
            }
            Ok((_, _, Err(e))) => {
                let _ = tx.send(ExecOutputEvent::Error(e.to_string())).await;
            }
            Err(_elapsed) => {
                kill_process_tree(pid).await;
                let duration_ms = start.elapsed().as_millis();
                self.metrics
                    .record_exec(duration_ms.try_into().unwrap_or(u64::MAX), true);
                let _ = tx
                    .send(ExecOutputEvent::Done {
                        exit_code: -1,
                        timed_out: true,
                        duration_ms,
                    })
                    .await;
            }
        }

        Ok(())
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
        self.metrics.record_file_write();
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

        let bytes = tokio::fs::read(&target)
            .await
            .map_err(|e| self.io_error("reading file", e))?;
        self.metrics.record_file_read();
        Ok(bytes)
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
        self.metrics.record_remove();
        Ok(())
    }

    async fn cleanup_expired(&self) -> Result<usize, LightSandboxError> {
        self.metrics.record_gc_run();
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
        self.metrics.record_gc_removed(processed as u64);
        Ok(processed)
    }

    async fn metrics(&self) -> Result<MetricsSnapshot, LightSandboxError> {
        Ok(self.metrics.snapshot(self.sandboxes.len() as u64))
    }
}
