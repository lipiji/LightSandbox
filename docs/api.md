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

## `GET /metrics`

Prometheus exposition endpoint. Returns runtime counters and gauges as a
`text/plain; version=0.0.4` body, ready to be scraped by Prometheus (or any
compatible collector) with no extra configuration.

Exposed series:

| Metric | Type | Meaning |
|---|---|---|
| `lightsandbox_sandboxes_created_total` | counter | Sandboxes created since start |
| `lightsandbox_sandboxes_active` | gauge | Sandboxes currently tracked |
| `lightsandbox_sandboxes_removed_total` | counter | Sandboxes explicitly removed |
| `lightsandbox_exec_total` | counter | `exec` calls that completed (incl. timeout) |
| `lightsandbox_exec_timed_out_total` | counter | `exec` calls that hit their timeout |
| `lightsandbox_exec_duration_seconds` | histogram | `exec` wall-clock duration |
| `lightsandbox_gc_runs_total` | counter | `cleanup_expired` invocations |
| `lightsandbox_gc_removed_total` | counter | Sandboxes reaped by GC |
| `lightsandbox_file_writes_total` | counter | Successful `write_file` calls |
| `lightsandbox_file_reads_total` | counter | Successful `read_file` calls |

Example scrape (`curl http://127.0.0.1:8080/metrics`):

```text
# HELP lightsandbox_exec_total Total exec calls that completed (normally or by timeout).
# TYPE lightsandbox_exec_total counter
lightsandbox_exec_total 2
# HELP lightsandbox_exec_duration_seconds Exec wall-clock duration in seconds.
# TYPE lightsandbox_exec_duration_seconds histogram
lightsandbox_exec_duration_seconds_bucket{le="0.05"} 2
lightsandbox_exec_duration_seconds_bucket{le="+Inf"} 2
lightsandbox_exec_duration_seconds_sum 0.056
lightsandbox_exec_duration_seconds_count 2
```

## `POST /v1/sandboxes`

Create a sandbox.

Request:
```json
{
  "ttl_seconds": 600,
  "env": {"PYTHONUNBUFFERED": "1"},
  "metadata": {"agent_id": "demo-agent"},
  "template": "python-ml"
}
```

`template` (optional) names a subdirectory under the server's `templates_dir`
(see config `[templates] dir`). If set, the template's contents are copied into
the new workspace at create time, so the sandbox starts with those files
already present instead of empty. An unknown template, or any template when
templates are not configured, returns `INVALID_PATH`.

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

## `POST /v1/sandboxes/{id}/exec/stream`

Like `exec`, but streams stdout/stderr as [Server-Sent Events](https://developer.mozilla.org/en-US/docs/Web/API/Server-sent_events/Using_server-sent_events) (`text/event-stream`) instead of buffering the whole result. Same request body as `exec`. A missing/expired sandbox still returns the normal JSON error envelope (checked before the stream opens); any failure that happens after the process has started is reported in-band as an `error` event instead.

Event types:

| `event:` | `data:` | Meaning |
|---|---|---|
| `stdout` | raw text chunk | A piece of stdout as it's produced |
| `stderr` | raw text chunk | A piece of stderr as it's produced |
| `done` | `{"exit_code":0,"timed_out":false,"duration_ms":123}` | Terminal event on normal completion or timeout |
| `error` | message string | Terminal event if the process could not be observed to completion after starting |

Each stream ends with exactly one `done` or `error` event. Multi-line chunks are sent as repeated `data:` lines per the SSE spec — join them with `\n` to reconstruct the original text.

Example (`curl --no-buffer`):

```text
$ curl -N -X POST http://127.0.0.1:8080/v1/sandboxes/sbx_abc123/exec/stream \
    -d '{"cmd":"python -u -c \"import time; [print(i) or time.sleep(0.3) for i in range(3)]\""}'

event: stdout
data: 0

event: stdout
data: 1

event: stdout
data: 2

event: done
data: {"exit_code":0,"timed_out":false,"duration_ms":912}
```

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

## `POST /v1/sandboxes/{id}/files/upload`

Binary-safe file upload via `multipart/form-data`. The JSON `PUT /files`
endpoint above round-trips file content through a UTF-8 string, so it cannot
carry arbitrary bytes (images, archives, etc.) losslessly — use this endpoint
for anything that isn't guaranteed to be valid UTF-8 text.

Fields:

| Field | Required | Meaning |
|---|---|---|
| `file` | yes | The file content (any bytes) |
| `path` | no | Destination path inside the workspace. Falls back to the `file` field's `filename` if omitted. |

Same path rules as `PUT /files` (`../` and absolute paths rejected with
`INVALID_PATH`); oversized uploads return `FILE_TOO_LARGE`.

```bash
curl -X POST http://127.0.0.1:8080/v1/sandboxes/sbx_abc123/files/upload \
  -F path=data/photo.png \
  -F file=@photo.png
```

Response:
```json
{"written": true, "path": "data/photo.png"}
```

## `GET /v1/sandboxes/{id}/files/download?path=main.bin`

Binary-safe file download. Returns the raw bytes with
`Content-Type: application/octet-stream` and
`Content-Disposition: attachment; filename="..."` instead of wrapping them in
a JSON string. Same path rules and error codes as `GET /files`.

```bash
curl -OJ http://127.0.0.1:8080/v1/sandboxes/sbx_abc123/files/download?path=data/photo.png
```

## `DELETE /v1/sandboxes/{id}`

Remove a sandbox and its workspace.

Response:
```json
{"removed": true}
```

## Templates

A template is just a directory under `templates_dir` (set via
`[templates] dir` in the config):

```text
data/templates/
  python-ml/        # template name = "python-ml"
    requirements.txt
    helper.py
    lib/
      util.py
```

`POST /v1/sandboxes` with `{"template": "python-ml"}` recursively copies that
directory into the new workspace. Templates are operator-placed and trusted
(like the workspace root) — they are not a sandboxing boundary. Comment out the
`[templates]` section to disable template support entirely.

## Warm pool (optional)

`[pool] enabled = true, min_idle = N` pre-builds `N` bare workspace
directories at startup and hands them out to template-less creates, refilling
lazily in the background. Pooled slots are invisible to `GET /v1/sandboxes`
and exempt from TTL/GC until handed out. It is **off by default**:
`LocalProcessRuntime` creates are already microsecond-scale, so this mainly
reserves the interface for future runtimes (Docker/Firecracker) where
cold-start cost is real.
