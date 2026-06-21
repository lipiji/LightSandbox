"""Shared fixtures: spins up a real lightsandbox-server.exe per test session.

Tests run against the actual server binary (not a mock) so the SDK is
verified against the real HTTP/JSON contract.
"""

from __future__ import annotations

import socket
import subprocess
import sys
import tempfile
import time
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parents[3]


def _free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def _server_binary() -> Path:
    exe = REPO_ROOT / "target" / "debug" / "lightsandbox-server.exe"
    if not exe.exists():
        exe = REPO_ROOT / "target" / "debug" / "lightsandbox-server"
    if not exe.exists():
        pytest.skip(f"lightsandbox-server binary not found at {exe}; run `cargo build` first")
    return exe


def _wait_for_port(port: int, timeout: float = 10.0) -> None:
    deadline = time.time() + timeout
    while time.time() < deadline:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
            if s.connect_ex(("127.0.0.1", port)) == 0:
                return
        time.sleep(0.05)
    raise TimeoutError(f"server did not open port {port} within {timeout}s")


@pytest.fixture()
def server_base_url(tmp_path: Path):
    port = _free_port()
    workspace_root = tmp_path / "data"
    config_path = tmp_path / "config.toml"
    config_path.write_text(
        f"""
[server]
host = "127.0.0.1"
port = {port}

[runtime]
type = "local"
workspace_root = "{workspace_root.as_posix()}"

[limits]
max_sandboxes = 100
max_concurrent_exec = 20
default_ttl_seconds = 600
default_exec_timeout_seconds = 60
max_stdout_bytes = 1048576
max_stderr_bytes = 1048576
max_file_size_bytes = 10485760
max_read_file_bytes = 10485760

[gc]
enabled = false
interval_seconds = 30
remove_expired = true

[security]
allow_absolute_paths = false
allow_path_traversal = false
hide_host_paths = true
"""
    )

    proc = subprocess.Popen(
        [str(_server_binary()), "--config", str(config_path)],
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    try:
        _wait_for_port(port)
        yield f"http://127.0.0.1:{port}"
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()


@pytest.fixture()
def python_exe() -> str:
    """Resolves a real python.exe, skipping the Windows Store stub."""
    if sys.platform != "win32":
        return "python3"
    import os

    for directory in os.environ.get("PATH", "").split(os.pathsep):
        if "windowsapps" in directory.lower():
            continue
        candidate = Path(directory) / "python.exe"
        if candidate.is_file():
            return str(candidate)
    pytest.skip("no usable python.exe found on PATH")
