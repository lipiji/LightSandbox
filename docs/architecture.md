# Architecture

LightSandbox is split into small, focused pieces connected by one trait:

```text
                 +-------------------+
  HTTP clients -->|    API Server     |
  (CLI, SDK,      |  (axum, api.rs)   |
   curl, agents)  +---------+---------+
                             |
                             v
                   +-------------------+
                   |   Runtime Trait    |
                   | (SandboxRuntime)   |
                   +---------+---------+
                             |
                             v
                   +-------------------+
                   | LocalProcessRuntime|
                   +----+----------+---+
                        |          |
                        v          v
              +------------+  +------------+
              | Workspace   |  | Process    |
              | Manager     |  | Executor   |
              +------------+  +------------+

                   +-------------------+
                   |     GC Task        |
                   | (background loop)  |
                   +-------------------+
```

## Components

- **API Server** (`crates/lightsandbox-server`): an axum HTTP server exposing the REST API described in [api.md](api.md). It holds an `Arc<dyn SandboxRuntime>` and never talks to the filesystem or processes directly — everything goes through the trait.
- **Runtime Trait** (`crates/lightsandbox-core::runtime::SandboxRuntime`): the single abstraction every backend implements (`create`, `list`, `get`, `exec`, `write_file`, `read_file`, `remove`, `cleanup_expired`). This is what makes the server, CLI, and SDK runtime-agnostic — swapping `LocalProcessRuntime` for a future `DockerRuntime` requires no API changes.
- **LocalProcessRuntime** (`crates/lightsandbox-runtime-local`): v0.1's only runtime. Combines a **Workspace Manager** (one directory per sandbox, path-traversal-safe read/write, TTL bookkeeping) and a **Process Executor** (spawns commands via `tokio::process`, enforces timeouts and output size caps).
- **GC Task** (`gc.rs` in the server): a periodic background task that calls `cleanup_expired()` on the runtime to remove sandboxes past their TTL.
- **Python SDK** (`sdk/python/lightsandbox`): a thin HTTP client over the REST API.
- **CLI** (`crates/lightsandbox-cli`): a thin HTTP client over the REST API, mirroring the SDK's operations for shell/scripting use.

## Why a trait instead of an enum or feature flags

v0.1 only ships `LocalProcessRuntime`, but the project's stated goal is to add `DockerRuntime`, `ContainerdRuntime`, `KubernetesRuntime`, `FirecrackerRuntime`, and others later without disrupting the API surface. A `SandboxRuntime` trait behind an `Arc<dyn SandboxRuntime>` in server state means a new runtime is a new crate implementing the trait, not a rewrite of `api.rs`.
