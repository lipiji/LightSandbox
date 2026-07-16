# LightSandbox

![CI](https://github.com/lipiji/LightSandbox/actions/workflows/ci.yml/badge.svg)
![Release](https://github.com/lipiji/LightSandbox/actions/workflows/release.yml/badge.svg)

**Self-hosted sandbox execution for AI agents. Run anywhere — laptop to cloud.**

LightSandbox is a REST API server that gives AI agents (Claude Code, Codex, or any HTTP client) isolated workspaces to execute commands, read/write files, and manage sandbox lifecycles. Deploy it on your laptop, a VPS, or a Kubernetes cluster — agents connect over HTTP and never touch your host directly.

> **Status: v0.2.** See [ROADMAP.md](ROADMAP.md) for what's implemented vs. planned, and [docs/security.md](docs/security.md) before running untrusted code.

## Why LightSandbox

Existing options are either a raw `subprocess.run()` call (no lifecycle, no limits, no cleanup) or a full cloud sandbox API (E2B, Daytona) that requires an account and sends your code to someone else's infrastructure.

LightSandbox aims for the middle: more engineered than raw subprocess, lighter than Docker, fully self-hostable, and E2B API-compatible so existing agent tooling drops right in.

## Install

### macOS

```bash
brew tap lipiji/lightsandbox
brew install lightsandbox
```

### Linux

```bash
curl -fsSL https://raw.githubusercontent.com/lipiji/LightSandbox/master/scripts/install.sh | sh
```

### Windows

```powershell
iwr https://raw.githubusercontent.com/lipiji/LightSandbox/master/scripts/install.ps1 | iex
```

### Download a binary directly

Pre-built binaries for every platform are attached to each [GitHub Release](https://github.com/lipiji/LightSandbox/releases):

| Platform | File |
|----------|------|
| Linux x86_64 (static) | `lightsandbox-server-linux-x86_64` |
| Linux arm64 (static) | `lightsandbox-server-linux-arm64` |
| macOS Apple Silicon | `lightsandbox-server-macos-arm64` |
| macOS Intel | `lightsandbox-server-macos-x86_64` |
| Windows x86_64 | `lightsandbox-server-windows-x86_64.exe` |

### Build from source

```bash
cargo build --release -p lightsandbox-server
```

Requires Rust stable. No Docker, no C toolchain, no system dependencies.

## Quick start

```bash
# start the server — no config file needed
lightsandbox-server

# or from source
cargo run -p lightsandbox-server
```

The server starts on `127.0.0.1:8080` by default, creates `./data/workspaces/` automatically, and is ready immediately:

```bash
curl http://127.0.0.1:8080/health   # {"status":"ok"}
```

### Config (optional)

Drop a `lightsandbox.toml` in your project directory to override any defaults — the server picks it up automatically:

```toml
[server]
host = "0.0.0.0"   # expose to the network for remote agents
port = 8080

[runtime]
workspace_root = "/var/lib/lightsandbox/workspaces"

[limits]
max_sandboxes = 200
default_ttl_seconds = 1800
```

All fields are optional — omit any section to keep the built-in default. See [`config.example.toml`](config.example.toml) for the full reference.

### End-to-end demo

```bash
# create → write → exec → read → remove
CLI="lightsandbox"                  # or: cargo run -q -p lightsandbox-cli --
ID=$($CLI --json create | grep -oE '"id":"sbx_[0-9a-f]+"' | cut -d'"' -f4)
$CLI write "$ID" README.md note.md
$CLI --json exec "$ID" "echo hello from inside the sandbox"
$CLI --json read "$ID" note.md
$CLI --json rm "$ID"
```

Works on macOS, Linux, and Git Bash on Windows. The Python SDK walkthrough (streaming exec, binary upload/download) is in [docs/quickstart.md](docs/quickstart.md).

## Deployment

LightSandbox is a single stateless binary — deploy it wherever agents can reach it:

| Scenario | How |
|----------|-----|
| Local dev | `lightsandbox-server` — binds `127.0.0.1:8080` |
| Cloud VPS | Set `host = "0.0.0.0"` in config, open port 8080 |
| Docker | `docker run -p 8080:8080 lipiji/lightsandbox` _(coming soon)_ |
| systemd service | See [docs/deployment.md](docs/quickstart.md) |

Agents connect with the Python SDK or any HTTP client by pointing at the server URL:

```python
from lightsandbox import LightSandboxClient

# local
client = LightSandboxClient("http://127.0.0.1:8080")

# remote
client = LightSandboxClient("http://your-server.example.com:8080")
```

## Core features

- **Zero-config startup**: download the binary and run it — no config file, no database, no Docker daemon.
- **Run anywhere**: localhost, VPS, cloud VM, or Kubernetes — same binary, same API.
- **Sandbox lifecycle**: create, list, get, exec, read/write files, remove — with TTL and background GC.
- **Concurrency-aware**: designed for many agents creating sandboxes and running commands simultaneously.
- **REST API + Python SDK + CLI**, all on the same HTTP surface.
- **E2B-compatible API subset**: drop-in replacement for the E2B control plane at `/e2b/sandboxes`.
- **Templates**: start a sandbox pre-populated from a named template directory — no per-sandbox file upload churn.
- **Observable**: Prometheus `/metrics` endpoint with sandbox/exec/GC counters and exec-duration histogram.
- **Runtime-agnostic core**: `LocalProcessRuntime` today; Docker, Firecracker, and Kubernetes runtimes behind the same `SandboxRuntime` trait in upcoming releases.

## How it compares

| | LightSandbox | E2B | Docker-based sandbox | Raw subprocess |
|---|---|---|---|---|
| Install | Single binary | Cloud account | Docker daemon | Nothing |
| Self-hostable | Yes | No | Yes | Yes |
| Isolation | Process (v0.2), Docker/VM (roadmap) | Strong (cloud microVM) | Container-level | None |
| E2B API compatible | Yes | — | No | No |
| Concurrency | High | High | Medium | Manual |
| Best for | Self-hosted agent infra, local dev, private deployments | Untrusted code, no local infra | Semi-trusted self-hosted | Trusted scripts, no lifecycle needed |

## REST API

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

## Python SDK

```python
from lightsandbox import LightSandboxClient

client = LightSandboxClient("http://127.0.0.1:8080")

with client.create(ttl_seconds=300) as sbx:
    sbx.write_file("main.py", "print('hello lightsandbox')")
    result = sbx.exec("python main.py")
    print(result.stdout)
```

Install: `pip install lightsandbox`

## CLI

```bash
lightsandbox server                         # start the server
lightsandbox create --json
lightsandbox exec sbx_xxx "python -V"
lightsandbox write sbx_xxx ./local.py main.py
lightsandbox read sbx_xxx main.py
lightsandbox rm sbx_xxx
```

## Security boundary

`LocalProcessRuntime` is for **trusted workloads and local AI agent development**. It provides workspace isolation, path-traversal checks, timeouts, output limits, and TTL/GC — but not OS-level sandboxing. A process running inside can still reach the host filesystem and network.

For untrusted code, use the upcoming Docker or Firecracker runtime (v0.3+).

See [docs/security.md](docs/security.md) for the full threat model.

## Roadmap

See [ROADMAP.md](ROADMAP.md).

## License

MIT — see [LICENSE](LICENSE).
