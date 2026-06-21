"""Demonstrates general-purpose code execution through LightSandbox: running
an arbitrary snippet of user-supplied code and reading back stdout/stderr.

Usage:
    cargo run -p lightsandbox-server &           # start the server first
    python examples/code_execution_demo/main.py
"""

from __future__ import annotations

import sys

from lightsandbox import LightSandboxClient

BASE_URL = sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1:8080"

SNIPPETS = {
    "fibonacci.py": (
        "def fib(n):\n"
        "    a, b = 0, 1\n"
        "    for _ in range(n):\n"
        "        a, b = b, a + b\n"
        "    return a\n"
        "\n"
        "print([fib(i) for i in range(10)])\n"
    ),
    "broken.py": "raise ValueError('this snippet is supposed to fail')\n",
}


def main() -> None:
    client = LightSandboxClient(BASE_URL)

    with client.create(ttl_seconds=120) as sbx:
        print(f"created sandbox: {sbx.id}")

        for filename, code in SNIPPETS.items():
            sbx.write_file(filename, code)
            result = sbx.exec(f"{sys.executable} {filename}")
            print(f"--- {filename} ---")
            print(f"exit_code={result.exit_code} timed_out={result.timed_out}")
            if result.stdout:
                print(f"stdout: {result.stdout.strip()}")
            if result.stderr:
                print(f"stderr: {result.stderr.strip()}")

    print(f"removed sandbox: {sbx.id}")


if __name__ == "__main__":
    main()
