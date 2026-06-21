# E2B-compatible API subset

LightSandbox exposes a lifecycle-only subset of [E2B](https://e2b.dev)'s
control-plane REST API under `/e2b/sandboxes`, alongside (not replacing) the
native `/v1/sandboxes` API. The goal is path- and field-name compatibility
for sandbox lifecycle calls, so tooling written against E2B's control plane
needs minimal changes to point at a LightSandbox server.

## Scope: lifecycle only

E2B's real API has two layers:

1. A **control plane** (`api.e2b.dev`) for `create` / `list` / `get` / `kill`
   / extend-`timeout` — this is what `/e2b/sandboxes` mirrors.
2. A per-sandbox **`envd` protocol** (separate subdomain per sandbox) for
   exec and filesystem operations — not public/stable enough to replicate
   faithfully, and architecturally different from LightSandbox's single
   shared server.

So `/e2b/sandboxes` covers lifecycle only. Use LightSandbox's native
`POST /v1/sandboxes/{id}/exec` and `/files` endpoints (see [api.md](api.md))
for exec and file I/O — they are not reimplemented under envd semantics.

## Routes

| Method | Path | Maps to | Notes |
|---|---|---|---|
| `POST` | `/e2b/sandboxes` | `SandboxRuntime::create` | 201 + sandbox object |
| `GET` | `/e2b/sandboxes` | `SandboxRuntime::list` | 200 + array |
| `GET` | `/e2b/sandboxes/{id}` | `SandboxRuntime::get` | 404 if missing |
| `DELETE` | `/e2b/sandboxes/{id}` | `SandboxRuntime::remove` | 204 No Content |
| `POST` | `/e2b/sandboxes/{id}/timeout` | `SandboxRuntime::extend_ttl` | 204 No Content |

## Field mapping

Request body for `POST /e2b/sandboxes`:

| E2B field | LightSandbox field | Notes |
|---|---|---|
| `templateID` | `template` | Must name an existing template dir; unknown template returns `INVALID_PATH` |
| `metadata` | `metadata` | Passed through unchanged |
| `envVars` | `env` | Passed through unchanged |
| `timeout` | `ttl_seconds` | Seconds; defaults to the server's `default_ttl_seconds` if omitted |

Response object (`create`/`list`/`get`):

| E2B field | Source |
|---|---|
| `sandboxID` | `SandboxInfo.id` |
| `templateID` | Only known at create time (echoed from the request); **`null` on `list`/`get`** since `SandboxInfo` doesn't persist which template a sandbox was created from |
| `metadata` | `SandboxInfo.metadata` |
| `startedAt` | `SandboxInfo.created_at` |
| `endAt` | `SandboxInfo.expires_at` |

`POST /e2b/sandboxes/{id}/timeout` body: `{"timeout": <seconds>}` — sets
(does not add to) the sandbox's expiry to `now + timeout`, matching E2B's
own timeout semantics.

## Known limitations

- **Lifecycle only.** No envd-equivalent exec/filesystem layer; use the
  native API for those.
- **`templateID` is `null` after the fact.** It's only known at create time
  in this subset, not persisted on the sandbox record.
- **Errors use LightSandbox's own envelope**, not E2B's: every error is
  `{"error": {"code", "message"}}` (see [api.md](api.md)) with LightSandbox's
  status codes, rather than replicating E2B's distinct error shape. This is
  a deliberate simplification — exact error-shape fidelity wasn't judged
  worth the added complexity for a lifecycle-only subset.
- **No `cpuCount`/`memoryMB`.** Real E2B responses include these; they're
  omitted entirely here rather than reporting numbers LightSandbox doesn't
  actually enforce per-sandbox.
