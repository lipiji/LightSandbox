# Roadmap

## v0.1
- LocalProcessRuntime
- REST API
- Python SDK
- CLI
- Workspace manager
- Exec timeout
- File read/write
- TTL and GC
- Basic concurrency control
- Process tree cleanup on timeout (Unix process group / Windows taskkill)
- Prometheus `/metrics` endpoint

## v0.2
- Streaming exec output
- Sandbox templates + warm pool (in-process — no Kubernetes needed)
- E2B-compatible API subset (pure API-shape work, no new runtime dependency)
- SQLite metadata persistence
- File upload/download multipart

## v0.3
- Optional DockerRuntime
- Rootless Docker guide
- Basic network policy
- Resource limit improvements

## v0.4
- Containerd runtime
- Authentication token

## v0.5
- Kubernetes runtime
- Multi-node scheduling

## v0.6
- Firecracker or microVM runtime
- Strong isolation profile
