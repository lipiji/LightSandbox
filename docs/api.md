# REST API

All requests/responses are JSON. All errors use the same envelope:

```json
{
  "error": {
    "code": "SANDBOX_NOT_FOUND",
    "message": "sandbox not found"
  }
}
```

Error codes: `SANDBOX_NOT_FOUND`, `SANDBOX_EXPIRED`, `INVALID_PATH`, `EXEC_TIMEOUT`, `EXEC_FAILED`, `FILE_TOO_LARGE`, `OUTPUT_TOO_LARGE`, `RUNTIME_ERROR`, `CONFIG_ERROR`, `INTERNAL_ERROR`.

Workspace paths in responses are logical (e.g. `/workspace`), never the real host path.

## `GET /health`

Response:
```json
{"status": "ok"}
```

## `POST /v1/sandboxes`

Create a sandbox.

Request:
```json
{
  "ttl_seconds": 600,
  "env": {"PYTHONUNBUFFERED": "1"},
  "metadata": {"agent_id": "demo-agent"}
}
```

Response:
```json
{
  "id": "sbx_xxx",
  "status": "running",
  "workspace_path": "/workspace",
  "created_at": "...",
  "expires_at": "..."
}
```

## `GET /v1/sandboxes`

List all sandboxes. Response: an array of the same shape as the create response.

## `GET /v1/sandboxes/{id}`

Get a single sandbox's info. `404` with `SANDBOX_NOT_FOUND` if it doesn't exist.

## `POST /v1/sandboxes/{id}/exec`

Execute a command inside the sandbox's workspace.

Request:
```json
{
  "cmd": "python -c \"print('hello lightsandbox')\"",
  "timeout_seconds": 30
}
```

Response:
```json
{
  "exit_code": 0,
  "stdout": "hello lightsandbox\n",
  "stderr": "",
  "duration_ms": 123,
  "timed_out": false
}
```

A command that exceeds `timeout_seconds` is forcibly terminated; the response has `timed_out: true` rather than an error envelope.

## `PUT /v1/sandboxes/{id}/files`

Write a file inside the sandbox's workspace. Parent directories are created automatically. Paths must stay inside the workspace (`../` and, by default, absolute paths are rejected with `INVALID_PATH`). Oversized writes return `FILE_TOO_LARGE`.

Request:
```json
{
  "path": "main.py",
  "content": "print('hello lightsandbox')"
}
```

## `GET /v1/sandboxes/{id}/files?path=main.py`

Read a file from the sandbox's workspace. Same path rules as write. Oversized reads return `OUTPUT_TOO_LARGE`. Missing files return `INVALID_PATH`, not a raw 500.

Response:
```json
{
  "path": "main.py",
  "content": "print('hello lightsandbox')"
}
```

## `DELETE /v1/sandboxes/{id}`

Remove a sandbox and its workspace.

Response:
```json
{"removed": true}
```
