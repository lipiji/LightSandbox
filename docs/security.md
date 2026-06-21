# Security

## LocalProcessRuntime is not a strong isolation boundary

LocalProcessRuntime is designed for trusted workloads and local AI agent development. For untrusted code execution, use DockerRuntime, gVisor, Firecracker, or another stronger isolation backend.

It is suitable for:

- Local AI agent development
- Trusted code execution
- Research script execution
- Private tool calls
- Internal automation tasks

It is **not** suitable for running untrusted user code directly.

Because LocalProcessRuntime ultimately runs a normal OS process, that process can, in principle, still:

- Access any file the host process can access
- Access the network
- Spawn further subprocesses
- Consume CPU or memory beyond what a single sandbox "should" use
- Invoke arbitrary system commands

LightSandbox v0.1's goal is to be lightweight and manageable, not to replace Docker, gVisor, Firecracker, or KVM-based isolation.

## What LightSandbox does mitigate

Within that boundary, LightSandbox still enforces real engineering controls so that trusted-but-imperfect code behaves predictably:

- **Workspace isolation**: every sandbox gets its own directory; file reads/writes are confined to it.
- **Path traversal prevention**: `../`-style escapes and (by default) absolute paths are rejected before touching the filesystem.
- **Execution timeouts**: commands that exceed their timeout are forcibly terminated rather than left running.
- **Output size limits**: stdout/stderr are capped to prevent unbounded memory growth.
- **File size limits**: writes and reads are capped to prevent unbounded disk/memory usage.
- **TTL and GC**: sandboxes are automatically expired and cleaned up rather than accumulating indefinitely.
- **Host path hiding**: API responses expose a logical workspace path, not the real host filesystem path, by default (`hide_host_paths = true` in `config.example.toml`).

## When to use a different runtime

If your use case involves running code supplied by an untrusted end user (not the agent operator), do not rely on LocalProcessRuntime. Wait for, or build against, a stronger isolation runtime (Docker, gVisor, Firecracker, Kubernetes) once available — see [ROADMAP.md](../ROADMAP.md).
