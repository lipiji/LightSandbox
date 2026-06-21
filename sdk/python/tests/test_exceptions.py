from __future__ import annotations

import pytest

from lightsandbox.exceptions import (
    LightSandboxError,
    SandboxConfigError,
    SandboxExecError,
    SandboxExpired,
    SandboxFileTooLarge,
    SandboxInternalError,
    SandboxInvalidPath,
    SandboxNotFound,
    SandboxOutputTooLarge,
    SandboxRuntimeError,
    SandboxTimeout,
    error_from_response,
)

ALL_SERVER_ERROR_CODES = {
    "SANDBOX_NOT_FOUND": SandboxNotFound,
    "SANDBOX_EXPIRED": SandboxExpired,
    "INVALID_PATH": SandboxInvalidPath,
    "EXEC_TIMEOUT": SandboxTimeout,
    "EXEC_FAILED": SandboxExecError,
    "FILE_TOO_LARGE": SandboxFileTooLarge,
    "OUTPUT_TOO_LARGE": SandboxOutputTooLarge,
    "RUNTIME_ERROR": SandboxRuntimeError,
    "CONFIG_ERROR": SandboxConfigError,
    "INTERNAL_ERROR": SandboxInternalError,
}


@pytest.mark.parametrize("code,expected_type", list(ALL_SERVER_ERROR_CODES.items()))
def test_every_server_error_code_maps_to_a_specific_exception(code, expected_type):
    exc = error_from_response(code, "some message")
    assert isinstance(exc, expected_type)
    assert exc.code == code
    assert exc.message == "some message"


def test_unknown_code_falls_back_to_base_error():
    exc = error_from_response("SOMETHING_NEW", "msg")
    assert type(exc) is LightSandboxError
