from __future__ import annotations

import pytest

from lightsandbox import LightSandboxClient, SandboxNotFound


def test_create_and_list(server_base_url):
    client = LightSandboxClient(server_base_url)
    sbx = client.create(ttl_seconds=120)
    assert sbx.id.startswith("sbx_")

    ids = [info["id"] for info in client.list()]
    assert sbx.id in ids


def test_context_manager_removes_sandbox_on_exit(server_base_url):
    client = LightSandboxClient(server_base_url)
    with client.create(ttl_seconds=120) as sbx:
        sandbox_id = sbx.id

    ids = [info["id"] for info in client.list()]
    assert sandbox_id not in ids


def test_write_exec_read_round_trip(server_base_url, python_exe):
    client = LightSandboxClient(server_base_url)
    sbx = client.create(ttl_seconds=120)

    sbx.write_file("main.py", "print('hello from sdk test')")

    result = sbx.exec(f"{python_exe} main.py")
    assert result.exit_code == 0
    assert "hello from sdk test" in result.stdout
    assert not result.timed_out

    content = sbx.read_file("main.py")
    assert "hello from sdk test" in content

    sbx.remove()


def test_exec_after_remove_raises_sandbox_not_found(server_base_url):
    client = LightSandboxClient(server_base_url)
    sbx = client.create(ttl_seconds=120)
    sbx.remove()

    with pytest.raises(SandboxNotFound):
        sbx.exec("echo gone")


def test_path_traversal_is_rejected(server_base_url):
    client = LightSandboxClient(server_base_url)
    sbx = client.create(ttl_seconds=120)

    from lightsandbox import SandboxInvalidPath

    with pytest.raises(SandboxInvalidPath) as exc_info:
        sbx.write_file("../escape.txt", "nope")
    assert exc_info.value.code == "INVALID_PATH"


def test_connection_error_for_unreachable_server():
    from lightsandbox import LightSandboxConnectionError

    client = LightSandboxClient("http://127.0.0.1:1", timeout=1.0)
    with pytest.raises(LightSandboxConnectionError):
        client.list()
