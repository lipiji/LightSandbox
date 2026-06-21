"""Demonstrates the kind of workflow an AI agent would run against LightSandbox:
create a sandbox, write a Python file into it, execute it, read the result
back, then remove the sandbox.

Usage:
    cargo run -p lightsandbox-server &           # start the server first
    python examples/python_agent_demo/main.py
"""

from __future__ import annotations

import sys

from lightsandbox import LightSandboxClient

BASE_URL = sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1:8080"


def main() -> None:
    client = LightSandboxClient(BASE_URL)

    with client.create(ttl_seconds=120) as sbx:
        print(f"created sandbox: {sbx.id}")

        sbx.write_file("agent_task.py", "print('hello from the agent sandbox')")

        result = sbx.exec(f"{sys.executable} agent_task.py")
        print(f"exec result: exit_code={result.exit_code} stdout={result.stdout.strip()!r}")

        content = sbx.read_file("agent_task.py")
        print(f"read back file content: {content!r}")

    print(f"removed sandbox: {sbx.id}")


if __name__ == "__main__":
    main()
