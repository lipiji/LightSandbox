//! Docker-backed `SandboxRuntime` — v0.3, **scaffold**.
//!
//! ## Status
//!
//! The pure docker-CLI argument builders in this module are implemented and
//! unit-tested. The trait methods that actually spawn the `docker` binary and
//! parse its output are `todo!()` until a Docker daemon is available in the
//! dev environment to test them end-to-end — the project deliberately does
//! not ship untested runtime lifecycle code. When Docker is up, each
//! `todo!()` becomes a `tokio::process::Command::new("docker")` call using
//! the builders here, plus `#[ignore]` integration tests that spin up real
//! containers (so CI without Docker skips them).
//!
//! ## Design (each trait method → docker command)
//!
//! One sandbox maps to one container named `lightsandbox-<sandbox_id>`.
//! Sandbox metadata and TTL live in an in-memory registry (`DashMap`),
//! mirroring `LocalProcessRuntime` — Docker itself has no TTL notion.
//!
//! | method          | docker command                                                        |
//! |-----------------|-----------------------------------------------------------------------|
//! | `create`        | `docker run -d --name <name> [--env K=V]... <image> sleep infinity`   |
//! | `list` / `get`  | registry lookup (no docker call)                                      |
//! | `exec`          | `docker exec <name> sh -c <cmd>` (timeout via tokio + kill)           |
//! | `write_file`    | write host temp file, then `docker cp <tmp> <name>:<path>`            |
//! | `read_file`     | `docker cp <name>:<path> <tmp>`, then read the bytes                  |
//! | `remove`        | `docker rm -f <name>`                                                 |
//! | `cleanup_expired` | registry scan + `docker rm -f` for each expired id                  |
//!
//! ## Open decisions (validate against a real daemon)
//!
//! - **Keep-alive**: `sleep infinity` override vs trusting the image's CMD.
//!   `sleep infinity` needs coreutils; `busybox` images use `sleep` differently.
//! - **Exec-timeout kill**: how to kill only the exec'd process tree inside
//!   the container (not the whole container) on timeout.
//! - **File copy**: `docker cp` (preferred) vs `exec sh -c 'cat > path'`.
//! - **Workspace path semantics**: logical `/workspace` → container path.

use std::collections::HashMap;

use async_trait::async_trait;
use dashmap::DashMap;
use lightsandbox_core::{
    ExecOutputEvent, ExecRequest, ExecResult, LightSandboxError, MetricsSnapshot, RuntimeConfig,
    SandboxId, SandboxInfo, SandboxRuntime, SandboxSpec,
};
use tokio::sync::mpsc::Sender;

/// Tunables specific to the Docker backend. The shared bookkeeping (workspace
/// root, TTL, max_sandboxes, output caps, ...) comes from the `RuntimeConfig`
/// passed to [`DockerRuntime::new`].
#[derive(Debug, Clone)]
pub struct DockerRuntimeConfig {
    /// Image used for every sandbox, e.g. `"python:3.12-slim"`.
    pub image: String,
    /// If true, `docker pull` is run before the first create (image freshness).
    pub pull_before_create: bool,
}

/// A `SandboxRuntime` backed by the Docker CLI. Each sandbox is one container;
/// an in-memory registry tracks sandbox id → container metadata and TTL.
pub struct DockerRuntime {
    config: DockerRuntimeConfig,
    runtime_config: RuntimeConfig,
    /// sandbox_id → container bookkeeping. Typed properly when the daemon
    /// layer lands; the registry shape mirrors `LocalProcessRuntime`.
    /// Underscore-prefixed because nothing reads it yet (the trait methods
    /// are `todo!()`); renamed to `sandboxes` once the daemon layer wires it.
    _sandboxes: DashMap<String, ()>,
}

impl DockerRuntime {
    pub fn new(runtime_config: RuntimeConfig, config: DockerRuntimeConfig) -> Self {
        Self {
            config,
            runtime_config,
            _sandboxes: DashMap::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Pure docker-CLI argument builders — implemented and unit-tested now (they
// don't touch the daemon). The trait methods below consume these once the
// spawn/parse layer lands.
// ---------------------------------------------------------------------------

/// The container name for a sandbox id. The `lightsandbox-` prefix makes
/// ownership obvious in `docker ps` and lets cleanup grep for our containers.
pub(crate) fn container_name(sandbox_id: &str) -> String {
    format!("lightsandbox-{sandbox_id}")
}

/// `docker run` args (everything after the `docker` binary):
/// `run -d --name <name> [--env K=V]... <image> sleep infinity`.
///
/// Env keys are emitted in sorted order so the result is deterministic and
/// unit-testable regardless of `HashMap` iteration order.
pub(crate) fn run_args(
    config: &DockerRuntimeConfig,
    name: &str,
    env: &HashMap<String, String>,
) -> Vec<String> {
    let mut args = vec![
        "run".to_string(),
        // `-d` detaches and prints the container id on stdout (which `create`
        // parses to confirm the container started).
        "-d".to_string(),
        "--name".to_string(),
        name.to_string(),
    ];
    let mut keys: Vec<&String> = env.keys().collect();
    keys.sort();
    for k in keys {
        args.push("--env".to_string());
        args.push(format!("{k}={}", env[k]));
    }
    args.push(config.image.clone());
    // Keep the container alive so `docker exec` works later. The keep-alive
    // strategy is an open decision (see module docs); `sleep infinity` is the
    // default assumption, validated against a real daemon when spawn lands.
    args.push("sleep".to_string());
    args.push("infinity".to_string());
    args
}

/// `docker exec <name> sh -c <cmd>` — runs `cmd` through the container's shell
/// so shell features (pipes, globs, `$VAR`) behave as callers expect.
pub(crate) fn exec_args(name: &str, cmd: &str) -> Vec<String> {
    vec![
        "exec".to_string(),
        name.to_string(),
        "sh".to_string(),
        "-c".to_string(),
        cmd.to_string(),
    ]
}

/// `docker cp <host_src> <name>:<container_dest>` — copy a file INTO the sandbox.
pub(crate) fn cp_in_args(host_src: &str, name: &str, container_dest: &str) -> Vec<String> {
    vec![
        "cp".to_string(),
        host_src.to_string(),
        format!("{name}:{container_dest}"),
    ]
}

/// `docker cp <name>:<container_src> <host_dest>` — copy a file OUT of the sandbox.
pub(crate) fn cp_out_args(name: &str, container_src: &str, host_dest: &str) -> Vec<String> {
    vec![
        "cp".to_string(),
        format!("{name}:{container_src}"),
        host_dest.to_string(),
    ]
}

/// `docker rm -f <name>`.
pub(crate) fn rm_args(name: &str) -> Vec<String> {
    vec!["rm".to_string(), "-f".to_string(), name.to_string()]
}

// ---------------------------------------------------------------------------
// SandboxRuntime — each method builds its docker command via the helpers
// above (so the command shape is pinned and the helpers stay exercised), then
// defers the actual spawn + output parsing until Docker is available for
// end-to-end testing. The `todo!()` message echoes the intended command.
// ---------------------------------------------------------------------------

#[async_trait]
impl SandboxRuntime for DockerRuntime {
    async fn create(&self, spec: SandboxSpec) -> Result<SandboxInfo, LightSandboxError> {
        let id = SandboxId::new();
        let name = container_name(id.as_str());
        let env = spec.env.clone().unwrap_or_default();
        let args = run_args(&self.config, &name, &env);
        // TTL bookkeeping will use this; reading it now keeps the field live.
        let _default_ttl = self.runtime_config.limits.default_ttl_seconds;
        let _ = spec;
        todo!(
            "spawn `docker` with {args:?}; parse the container id from stdout; \
             register sandbox {id}; return SandboxInfo"
        )
    }

    async fn list(&self) -> Result<Vec<SandboxInfo>, LightSandboxError> {
        todo!("scan the in-memory registry and return SandboxInfo for each sandbox")
    }

    async fn get(&self, id: &str) -> Result<SandboxInfo, LightSandboxError> {
        let _ = container_name(id);
        todo!("look up {id} in the in-memory registry; SandboxNotFound if absent")
    }

    async fn exec(&self, id: &str, req: ExecRequest) -> Result<ExecResult, LightSandboxError> {
        let args = exec_args(&container_name(id), &req.cmd);
        todo!("spawn `docker` with {args:?}; enforce timeout; return ExecResult")
    }

    async fn exec_stream(
        &self,
        id: &str,
        req: ExecRequest,
        tx: Sender<ExecOutputEvent>,
    ) -> Result<(), LightSandboxError> {
        // `tx` is held by the streaming variant; bound underscore-prefixed so
        // it's not flagged unused until the spawn layer wires it up.
        let _tx = tx;
        let args = exec_args(&container_name(id), &req.cmd);
        todo!("spawn `docker` with {args:?}; pump stdout/stderr through the channel")
    }

    async fn write_file(
        &self,
        id: &str,
        path: &str,
        content: Vec<u8>,
    ) -> Result<(), LightSandboxError> {
        // Real impl: write `content` to a host tempfile, `docker cp` it in.
        let args = cp_in_args("<host-tmpfile>", &container_name(id), path);
        todo!(
            "write {} bytes to a tempfile, then spawn `docker` with {args:?}",
            content.len()
        )
    }

    async fn read_file(&self, id: &str, path: &str) -> Result<Vec<u8>, LightSandboxError> {
        let args = cp_out_args(&container_name(id), path, "<host-tmpfile>");
        todo!("spawn `docker` with {args:?}; read and return the bytes")
    }

    async fn extend_ttl(
        &self,
        id: &str,
        ttl_seconds: u64,
    ) -> Result<SandboxInfo, LightSandboxError> {
        let _ = container_name(id);
        todo!("set {id}'s expiry to now + {ttl_seconds}s in the registry; return SandboxInfo")
    }

    async fn remove(&self, id: &str) -> Result<(), LightSandboxError> {
        let args = rm_args(&container_name(id));
        todo!("spawn `docker` with {args:?}; drop the registry entry")
    }

    async fn cleanup_expired(&self) -> Result<usize, LightSandboxError> {
        todo!("scan the registry for expired ids; `docker rm -f` each; return the count")
    }

    async fn metrics(&self) -> Result<MetricsSnapshot, LightSandboxError> {
        todo!("return a snapshot of the runtime's counters (best-effort, never fails)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(image: &str) -> DockerRuntimeConfig {
        DockerRuntimeConfig {
            image: image.to_string(),
            pull_before_create: false,
        }
    }

    #[test]
    fn container_name_is_namespaced() {
        assert_eq!(container_name("sbx_abc123"), "lightsandbox-sbx_abc123");
    }

    #[test]
    fn run_args_shape_no_env() {
        let args = run_args(
            &cfg("python:3.12-slim"),
            "lightsandbox-sbx_x",
            &HashMap::new(),
        );
        assert_eq!(
            args,
            vec![
                "run",
                "-d",
                "--name",
                "lightsandbox-sbx_x",
                "python:3.12-slim",
                "sleep",
                "infinity",
            ]
        );
    }

    #[test]
    fn run_args_env_is_sorted_and_kv_encoded() {
        // Insert in non-sorted order; output must be sorted so the arg vector
        // is deterministic (testable) regardless of HashMap order.
        let mut env = HashMap::new();
        env.insert("ZETA".to_string(), "1".to_string());
        env.insert("ALPHA".to_string(), "2".to_string());
        env.insert("BETA=evil".to_string(), "3".to_string());
        let args = run_args(&cfg("alpine:3"), "lightsandbox-sbx_y", &env);
        // image + sleep follow the env block.
        assert_eq!(args[..4], vec!["run", "-d", "--name", "lightsandbox-sbx_y"]);
        let env_block = &args[4..args.len() - 3];
        assert_eq!(
            env_block,
            vec![
                "--env",
                "ALPHA=2",
                "--env",
                "BETA=evil=3",
                "--env",
                "ZETA=1",
            ]
        );
        assert_eq!(
            args[args.len() - 3..],
            vec!["alpine:3", "sleep", "infinity"]
        );
    }

    #[test]
    fn exec_args_uses_sh_c() {
        let args = exec_args("lightsandbox-sbx_x", "echo $HOME | wc -l");
        assert_eq!(
            args,
            vec![
                "exec",
                "lightsandbox-sbx_x",
                "sh",
                "-c",
                "echo $HOME | wc -l"
            ]
        );
    }

    #[test]
    fn cp_args_direction_and_format() {
        assert_eq!(
            cp_in_args("/tmp/x", "lightsandbox-sbx_x", "data/f.txt"),
            vec!["cp", "/tmp/x", "lightsandbox-sbx_x:data/f.txt"]
        );
        assert_eq!(
            cp_out_args("lightsandbox-sbx_x", "data/f.txt", "/tmp/y"),
            vec!["cp", "lightsandbox-sbx_x:data/f.txt", "/tmp/y"]
        );
    }

    #[test]
    fn rm_args_is_force() {
        assert_eq!(
            rm_args("lightsandbox-sbx_x"),
            vec!["rm", "-f", "lightsandbox-sbx_x"]
        );
    }

    #[test]
    fn new_runtime_holds_config() {
        // Smoke check that construction doesn't panic and stores the image.
        let rt = DockerRuntime::new(
            RuntimeConfig::default(),
            DockerRuntimeConfig {
                image: "alpine:3".to_string(),
                pull_before_create: true,
            },
        );
        assert_eq!(rt.config.image, "alpine:3");
        assert!(rt.config.pull_before_create);
    }
}
