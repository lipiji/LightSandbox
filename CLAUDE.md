# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project status

Implemented through v0.2: `LocalProcessRuntime`, REST API, Python SDK, CLI, streaming exec, sandbox templates + warm pool, an E2B-compatible API subset, restart-surviving metadata persistence (via `redb`), and binary file upload/download. The authoritative references for what exists today are the code itself, `README.md`, `ROADMAP.md`, and `docs/`. The notes below are historical design context and may lag the implementation as it evolves.

## What LightSandbox is

LightSandbox is a lightweight, high-concurrency sandbox execution layer for AI agents: a REST API + Python SDK + CLI for creating isolated workspaces, executing commands, reading/writing files, enforcing timeouts, and cleaning up agent tasks. v0.1 ships a zero-Docker `LocalProcessRuntime` for trusted local workloads; Docker/gVisor/Firecracker/Kubernetes runtimes are future, optional backends behind a common trait.

Core constraints (do not violate without discussing with the user first):
- No hard dependency on Docker, Kubernetes, a database, Redis, or a message queue in v0.1.
- No complex web UI, no heavy multi-tenant platform.
- `LocalProcessRuntime` is explicitly *not* a strong security isolation boundary — this must stay documented (README + `docs/security.md`), not just implied.
- Must run with zero external services on a machine without Docker installed.

## Architecture

Cargo workspace with these crates:
- `crates/lightsandbox-core` — shared models (`SandboxId`, `SandboxSpec`, `SandboxInfo`, `SandboxStatus`, `ExecRequest`, `ExecResult`, `FileWriteRequest`, `FileReadResponse`, `ResourceLimits`, `RuntimeConfig`, `LightSandboxError`) and the `SandboxRuntime` async trait (`create`, `list`, `get`, `exec`, `write_file`, `read_file`, `remove`, `cleanup_expired`). All runtimes implement this trait — new backends (Docker, Kubernetes, Firecracker, etc.) are added by implementing it, not by branching the API layer.
- `crates/lightsandbox-runtime-local` — `LocalProcessRuntime`: one workspace directory per sandbox under a configurable `workspace_root`, in-memory state (Arc/RwLock or DashMap) for concurrency, process exec with timeout + forced termination, stdout/stderr size caps, path-traversal-safe file read/write, TTL + background GC.
- `crates/lightsandbox-server` — axum HTTP server exposing the REST API (`api.rs`), shared state (`state.rs`), and the GC background task (`gc.rs`). The CLI and SDK talk to this server over HTTP — they never call the runtime directly.
- `crates/lightsandbox-cli` — thin CLI (`lightsandbox server|create|list|exec|write|read|rm`, with `--json` output) that calls the REST API.
- `sdk/python/lightsandbox` — Python client (`LightSandboxClient`, context-manager-capable sandbox handle) with its own exception hierarchy (`LightSandboxError`, `SandboxNotFound`, `SandboxExpired`, `SandboxTimeout`, `SandboxExecError`, `LightSandboxConnectionError`).
- `examples/` — `python_agent_demo`, `code_execution_demo`, `concurrent_sandboxes` (concurrency benchmark harness).
- `docs/quickstart.md`, `docs/api.md`, `docs/architecture.md`, `docs/security.md` — user-facing documentation.

API surface (REST, JSON, all errors as `{"error": {"code": ..., "message": ...}}`):
`GET /health`, `POST /v1/sandboxes`, `GET /v1/sandboxes`, `GET /v1/sandboxes/{id}`, `POST /v1/sandboxes/{id}/exec`, `PUT /v1/sandboxes/{id}/files`, `GET /v1/sandboxes/{id}/files?path=`, `DELETE /v1/sandboxes/{id}`. Workspace paths returned to clients must be logical (e.g. `/workspace`), never the real host path. Error codes are a fixed set (see `docs/api.md`).

## Tech stack

Rust + tokio + axum + serde/serde_json + clap + tracing; Python SDK via `requests`/`httpx`; TOML config (`config.example.toml` is the canonical config schema). Avoid adding dependencies beyond this set without good reason — the project's stated identity is "lighter than Docker, more engineered than raw subprocess."

## Commands

```bash
cargo build                          # build the workspace
cargo test                           # run all tests (must not depend on Docker)
cargo fmt                            # format before committing
cargo run -p lightsandbox-server     # start the API server (reads config.example.toml)
curl http://127.0.0.1:8080/health    # smoke test
python examples/python_agent_demo/main.py   # end-to-end demo via Python SDK
cargo run --example concurrent_sandboxes -- --n 100 --concurrency 20   # concurrency benchmark
```

## Test coverage

Creation, list visibility, `echo`/`python` exec, write-then-read file round trip, exec-after-removal rejection, timeout enforcement, path-traversal rejection (`../x`), oversized file rejection, GC of expired sandboxes, concurrent sandbox creation without crashing, stable API error shape. None of these may depend on Docker.

## Commit style

Conventional, e.g. `feat: implement local process runtime`, `test: add local runtime tests`, `docs: add quickstart and security notes`.
