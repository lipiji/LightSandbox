"""HTTP client for the LightSandbox REST API."""

from __future__ import annotations

import json
from typing import Any, Iterator

import requests

from .exceptions import (
    LightSandboxConnectionError,
    SandboxExecError,
    error_from_response,
)
from .sandbox import Sandbox


class LightSandboxClient:
    """Talks to a running lightsandbox-server over HTTP."""

    def __init__(self, base_url: str, timeout: float = 30.0):
        self.base_url = base_url.rstrip("/")
        self.timeout = timeout
        self._session = requests.Session()

    def create(
        self,
        ttl_seconds: int | None = None,
        metadata: dict[str, str] | None = None,
        env: dict[str, str] | None = None,
        template: str | None = None,
    ) -> Sandbox:
        payload = _drop_none(
            {
                "ttl_seconds": ttl_seconds,
                "metadata": metadata,
                "env": env,
                "template": template,
            }
        )
        data = self._request("POST", "/v1/sandboxes", json=payload)
        return Sandbox(self, data["id"], info=data)

    def list(self) -> list[dict[str, Any]]:
        return self._request("GET", "/v1/sandboxes")

    def get(self, sandbox_id: str) -> Sandbox:
        data = self._request("GET", f"/v1/sandboxes/{sandbox_id}")
        return Sandbox(self, sandbox_id, info=data)

    def remove(self, sandbox_id: str) -> None:
        self._request("DELETE", f"/v1/sandboxes/{sandbox_id}")

    def exec(
        self,
        sandbox_id: str,
        cmd: str,
        timeout_seconds: int | None = None,
        env: dict[str, str] | None = None,
    ) -> dict[str, Any]:
        payload = _drop_none({"cmd": cmd, "timeout_seconds": timeout_seconds, "env": env})
        return self._request("POST", f"/v1/sandboxes/{sandbox_id}/exec", json=payload)

    def exec_stream(
        self,
        sandbox_id: str,
        cmd: str,
        timeout_seconds: int | None = None,
        env: dict[str, str] | None = None,
    ) -> Iterator[tuple[str, Any]]:
        """Yields `("stdout", str)` / `("stderr", str)` chunks as the command
        runs, followed by exactly one `("done", dict)` with
        `exit_code`/`timed_out`/`duration_ms`. Raises `SandboxExecError` if
        the command fails after it has already started.
        """
        payload = _drop_none({"cmd": cmd, "timeout_seconds": timeout_seconds, "env": env})
        url = f"{self.base_url}/v1/sandboxes/{sandbox_id}/exec/stream"
        try:
            response = self._session.post(
                url, json=payload, timeout=self.timeout, stream=True
            )
        except requests.RequestException as exc:
            raise LightSandboxConnectionError(str(exc)) from exc

        if not response.ok:
            try:
                data = response.json()
            except ValueError as exc:
                raise LightSandboxConnectionError(f"invalid response body: {exc}") from exc
            error = data.get("error", {})
            raise error_from_response(
                error.get("code", "UNKNOWN"), error.get("message", "request failed")
            )

        yield from _parse_sse(response)

    def write_file(self, sandbox_id: str, path: str, content: str) -> None:
        self._request(
            "PUT",
            f"/v1/sandboxes/{sandbox_id}/files",
            json={"path": path, "content": content},
        )

    def read_file(self, sandbox_id: str, path: str) -> dict[str, Any]:
        return self._request(
            "GET", f"/v1/sandboxes/{sandbox_id}/files", params={"path": path}
        )

    def _request(
        self,
        method: str,
        path: str,
        json: dict[str, Any] | None = None,
        params: dict[str, Any] | None = None,
    ) -> Any:
        url = f"{self.base_url}{path}"
        try:
            response = self._session.request(
                method, url, json=json, params=params, timeout=self.timeout
            )
        except requests.RequestException as exc:
            raise LightSandboxConnectionError(str(exc)) from exc

        try:
            data = response.json()
        except ValueError as exc:
            raise LightSandboxConnectionError(f"invalid response body: {exc}") from exc

        if not response.ok:
            error = data.get("error", {})
            raise error_from_response(
                error.get("code", "UNKNOWN"), error.get("message", "request failed")
            )

        return data


def _drop_none(payload: dict[str, Any]) -> dict[str, Any]:
    return {k: v for k, v in payload.items() if v is not None}


def _parse_sse(response: requests.Response) -> Iterator[tuple[str, Any]]:
    """Hand-rolled SSE parser: groups lines into blank-line-delimited frames,
    joining repeated `data:` lines with `\\n` per the SSE spec. No
    third-party SSE dependency needed for a protocol this small.
    """
    event = ""
    data_lines: list[str] = []

    for raw_line in response.iter_lines(decode_unicode=False):
        if raw_line == b"":
            if event or data_lines:
                data = "\n".join(data_lines)
                if event == "stdout":
                    yield "stdout", data
                elif event == "stderr":
                    yield "stderr", data
                elif event == "done":
                    yield "done", json.loads(data)
                elif event == "error":
                    raise SandboxExecError(data)
                event, data_lines = "", []
            continue

        line = raw_line.decode("utf-8", errors="replace")
        if line.startswith("event:"):
            event = line[len("event:") :].lstrip(" ")
        elif line.startswith("data:"):
            value = line[len("data:") :]
            data_lines.append(value[1:] if value.startswith(" ") else value)
