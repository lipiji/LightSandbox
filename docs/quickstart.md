# Quickstart

## 1. Run the server

```bash
cargo run -p lightsandbox-server -- --config config.example.toml
```

## 2. Health check

```bash
curl http://127.0.0.1:8080/health
# {"status":"ok"}
```

## 3. Create a sandbox

```bash
curl -X POST http://127.0.0.1:8080/v1/sandboxes \
  -H "content-type: application/json" \
  -d '{"ttl_seconds": 600}'
```

Response:

```json
{
  "id": "sbx_xxxxxxxxxxxx",
  "status": "running",
  "workspace_path": "/workspace",
  "created_at": "...",
  "expires_at": "..."
}
```

## 4. Write a file

```bash
curl -X PUT http://127.0.0.1:8080/v1/sandboxes/sbx_xxxxxxxxxxxx/files \
  -H "content-type: application/json" \
  -d '{"path": "main.py", "content": "print(\"hello lightsandbox\")"}'
```

## 5. Execute it

```bash
curl -X POST http://127.0.0.1:8080/v1/sandboxes/sbx_xxxxxxxxxxxx/exec \
  -H "content-type: application/json" \
  -d '{"cmd": "python main.py", "timeout_seconds": 30}'
```

Response:

```json
{
  "exit_code": 0,
  "stdout": "hello lightsandbox\n",
  "stderr": "",
  "duration_ms": 42,
  "timed_out": false
}
```

## 6. Read the file back

```bash
curl "http://127.0.0.1:8080/v1/sandboxes/sbx_xxxxxxxxxxxx/files?path=main.py"
```

## 7. Remove the sandbox

```bash
curl -X DELETE http://127.0.0.1:8080/v1/sandboxes/sbx_xxxxxxxxxxxx
```

## Equivalent via the Python SDK

```python
from lightsandbox import LightSandboxClient

client = LightSandboxClient("http://127.0.0.1:8080")

with client.create(ttl_seconds=300) as sbx:
    sbx.write_file("main.py", "print('hello lightsandbox')")
    result = sbx.exec("python main.py")
    print(result.stdout)
```

## Equivalent via the CLI

```bash
lightsandbox create --json
lightsandbox write sbx_xxxxxxxxxxxx ./local_main.py main.py
lightsandbox exec sbx_xxxxxxxxxxxx "python main.py"
lightsandbox read sbx_xxxxxxxxxxxx main.py
lightsandbox rm sbx_xxxxxxxxxxxx
```

## Binary files (images, archives, blobs)

`write_file`/`read_file` (SDK) and `write`/`read` (CLI) are **text-only** —
they round-trip through JSON, so they can't represent arbitrary bytes. For
non-text payloads use the binary endpoints instead:

```python
with client.create(ttl_seconds=300) as sbx:
    sbx.upload("photo.bin", open("./photo.bin", "rb").read())
    blob = sbx.download("photo.bin")   # raw bytes, byte-identical to the input
```

```bash
lightsandbox upload sbx_xxxxxxxxxxxx ./photo.bin photo.bin
lightsandbox download sbx_xxxxxxxxxxxx photo.bin ./photo.copy.bin   # to a file
lightsandbox download sbx_xxxxxxxxxxxx photo.bin > photo.copy.bin   # to stdout
```
