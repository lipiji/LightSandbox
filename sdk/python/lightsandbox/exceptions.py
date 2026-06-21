"""Exception types raised by the LightSandbox Python SDK."""

from __future__ import annotations


class LightSandboxError(Exception):
    """Base class for all LightSandbox SDK errors."""

    def __init__(self, message: str, code: str | None = None):
        super().__init__(message)
        self.message = message
        self.code = code


class SandboxNotFound(LightSandboxError):
    """Raised when a sandbox id does not exist on the server."""


class SandboxExpired(LightSandboxError):
    """Raised when a sandbox's TTL has elapsed."""


class SandboxTimeout(LightSandboxError):
    """Raised when a command execution exceeds its timeout."""


class SandboxExecError(LightSandboxError):
    """Raised when the server fails to execute a command."""


class LightSandboxConnectionError(LightSandboxError):
    """Raised when the SDK cannot reach the LightSandbox server."""


_CODE_TO_EXCEPTION = {
    "SANDBOX_NOT_FOUND": SandboxNotFound,
    "SANDBOX_EXPIRED": SandboxExpired,
    "EXEC_TIMEOUT": SandboxTimeout,
    "EXEC_FAILED": SandboxExecError,
}


def error_from_response(code: str, message: str) -> LightSandboxError:
    """Maps a server error envelope `{code, message}` to an SDK exception."""
    exc_type = _CODE_TO_EXCEPTION.get(code, LightSandboxError)
    return exc_type(message, code=code)
