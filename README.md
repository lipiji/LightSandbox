# LightSandbox

LightSandbox is a lightweight sandbox runtime for AI agents.

It provides a simple REST API, Python SDK, and CLI for creating isolated workspaces, executing commands, reading/writing files, enforcing timeouts, and cleaning up agent tasks.

LightSandbox v0.1 starts with a zero-Docker `LocalProcessRuntime` for trusted local workloads. Stronger isolation backends such as Docker, gVisor, Firecracker, and Kubernetes are planned as optional runtimes.

> **Status: v0.1, work in progress.** See [ROADMAP.md](ROADMAP.md) for what's implemented vs. planned, and [docs/security.md](docs/security.md) before running untrusted code.

## Why LightSandbox

Existing AI agent sandboxes tend to be either a raw `subprocess.run()` call (no lifecycle management, no limits, no cleanup) or a full container/microVM platform (Docker, Kubernetes, Firecracker) that's heavy to install and operate for a single developer, a research project, or a private deployment.

LightSandbox aims for the middle: more engineered than raw subprocess, lighter than Docker, and more suited to local and private-first AI agent development than a cloud sandbox API.

## Core features

- **No required dependencies**: v0.1 runs with no Docker, Kubernetes, database, Redis, or message queue.
- **Sandbox lifecycle**: create, list, get, exec, read/write files, remove — with TTL and background GC.
- **Concurrency-aware**: designed for multiple agents creating sandboxes and running commands at the same time.
- **REST API, Python SDK, and CLI**, all built on the same HTTP surface.
- **Observable**: a Prometheus `/metrics` endpoint exposes sandbox/exec/GC counters and an exec-duration histogram, scrapable with no extra config.
- **Templates**: create a sandbox from a named template directory so it starts pre-populated with files/dependencies — no per-sandbox `write_file` churn.
- **Runtime-agnostic core**: a single `SandboxRuntime` trait, implemented today by `LocalProcessRuntime`, with `DockerRuntime`, `KubernetesRuntime`, `FirecrackerRuntime`, and others as future, optional backends behind the same interface.
- **Honest about its security boundary**: see below.

## How it compares

| | LightSandbox v0.1 | Docker-based sandbox | OpenSandbox / agent-sandbox | E2B | CubeSandbox |
|---|---|---|---|---|---|
| Install footprint | Just the binary | Docker daemon required | Container runtime required | Cloud-hosted | Hardware isolation, heavier infra |
| Isolation strength | None (trusted workloads only) | Container-level | Container-level | Strong (cloud microVM) | Strong (hardware-assisted) |
| Best for | Local agent dev, trusted code, private deployments | Self-hosted, semi-trusted code | Kubernetes-native agent workloads | Untrusted code, no local infra | High-concurrency, low cold-start untrusted code |

LightSandbox v0.1 does not try to match the isolation guarantees of the others — it trades isolation strength for installation simplicity and is meant to be replaced or supplemented with a stronger runtime when untrusted code execution is required.

## Quick start

```bash
# build and run the server
cargo run -p lightsandbox-server -- --config config.example.toml

# in another terminal
curl http://127.0.0.1:8080/health
# {"status":"ok"}
```

See [docs/quickstart.md](docs/quickstart.md) for the full create → write → exec → remove walkthrough.

## REST API example

```bash
# create a sandbox
curl -X POST http://127.0.0.1:8080/v1/sandboxes \
  -H "content-type: application/json" \
  -d '{"ttl_seconds": 600}'

# exec a command
curl -X POST http://127.0.0.1:8080/v1/sandboxes/sbx_xxx/exec \
  -H "content-type: application/json" \
  -d '{"cmd": "python -c \"print(1+1)\"", "timeout_seconds": 30}'
```

Full endpoint reference: [docs/api.md](docs/api.md).

## Python SDK example

```python
from lightsandbox import LightSandboxClient

client = LightSandboxClient("http://127.0.0.1:8080")

with client.create(ttl_seconds=300) as sbx:
    sbx.write_file("main.py", "print('hello lightsandbox')")
    result = sbx.exec("python main.py")
    print(result.stdout)
```

## CLI example

```bash
lightsandbox create --json
lightsandbox exec sbx_xxx "python -V"
lightsandbox write sbx_xxx ./local.py main.py
lightsandbox read sbx_xxx main.py
lightsandbox rm sbx_xxx
```

## LocalProcessRuntime security boundary

LocalProcessRuntime is designed for trusted workloads and local AI agent development. For untrusted code execution, use DockerRuntime, gVisor, Firecracker, or another stronger isolation backend.

It is **not** a strong security isolation environment. A process run by LocalProcessRuntime can, in principle, still access anything the host process can: the filesystem, the network, spawning further subprocesses, and host CPU/memory. LightSandbox manages it (workspace isolation, path traversal checks, timeouts, output limits, TTL/GC) but does not sandbox it at the OS/kernel level.

Use it for: local agent development, trusted code, research scripts, internal automation, private tool calls.
Do not use it to run code from untrusted users directly. See [docs/security.md](docs/security.md) for details.

## Roadmap

See [ROADMAP.md](ROADMAP.md).

## License

MIT — see [LICENSE](LICENSE).
