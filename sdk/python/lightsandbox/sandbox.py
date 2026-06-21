"""Sandbox handle returned by LightSandboxClient."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any


@dataclass
class ExecResult:
    exit_code: int
    stdout: str
    stderr: str
    duration_ms: int
    timed_out: bool

    @classmethod
    def from_dict(cls, data: dict) -> "ExecResult":
        return cls(
            exit_code=data["exit_code"],
            stdout=data["stdout"],
            stderr=data["stderr"],
            duration_ms=data["duration_ms"],
            timed_out=data["timed_out"],
        )


class Sandbox:
    """Handle to a single sandbox on a LightSandbox server.

    Supports `with client.create(...) as sbx:` usage, which removes the
    sandbox on exit, as well as plain non-context-manager usage where the
    caller calls `remove()` explicitly.
    """

    def __init__(self, client: Any, sandbox_id: str, info: dict | None = None):
        self._client = client
        self.id = sandbox_id
        self._info = info or {}

    @property
    def status(self) -> str:
        return self._info.get("status", "unknown")

    @property
    def workspace_path(self) -> str:
        return self._info.get("workspace_path", "")

    def exec(
        self,
        cmd: str,
        timeout_seconds: int | None = None,
        env: dict[str, str] | None = None,
    ) -> ExecResult:
        data = self._client.exec(self.id, cmd, timeout_seconds=timeout_seconds, env=env)
        return ExecResult.from_dict(data)

    def write_file(self, path: str, content: str) -> None:
        self._client.write_file(self.id, path, content)

    def read_file(self, path: str) -> str:
        data = self._client.read_file(self.id, path)
        return data["content"]

    def remove(self) -> None:
        self._client.remove(self.id)

    def __enter__(self) -> "Sandbox":
        return self

    def __exit__(self, exc_type: Any, exc: Any, tb: Any) -> None:
        self.remove()
