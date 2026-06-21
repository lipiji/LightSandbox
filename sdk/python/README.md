# lightsandbox (Python SDK)

Python client for [LightSandbox](../../README.md), talking to a running `lightsandbox-server` over its REST API.

## Install

```bash
pip install -e sdk/python
```

## Usage

```python
from lightsandbox import LightSandboxClient

client = LightSandboxClient("http://127.0.0.1:8080")

with client.create(ttl_seconds=300) as sbx:
    sbx.write_file("main.py", "print('hello lightsandbox')")
    result = sbx.exec("python main.py")
    print(result.stdout)
```

Non-context-manager usage:

```python
from lightsandbox import LightSandboxClient

client = LightSandboxClient("http://127.0.0.1:8080")

sbx = client.create()
sbx.write_file("main.py", "print('hello')")
result = sbx.exec("python main.py")
print(result.stdout)
sbx.remove()
```

## Exceptions

```
LightSandboxError
SandboxNotFound
SandboxExpired
SandboxTimeout
SandboxExecError
SandboxInvalidPath
SandboxFileTooLarge
SandboxOutputTooLarge
SandboxRuntimeError
SandboxConfigError
SandboxInternalError
LightSandboxConnectionError
```
