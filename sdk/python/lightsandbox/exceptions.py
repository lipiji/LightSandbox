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


class SandboxInvalidPath(LightSandboxError):
    """Raised when a requested file path is rejected (traversal, absolute, etc.)."""


class SandboxFileTooLarge(LightSandboxError):
    """Raised when a file write exceeds the server's size limit."""


class SandboxOutputTooLarge(LightSandboxError):
    """Raised when a file read exceeds the server's size limit."""


class SandboxRuntimeError(LightSandboxError):
    """Raised when the server's runtime layer fails unexpectedly."""


class SandboxConfigError(LightSandboxError):
    """Raised when the server reports a configuration error."""


class SandboxInternalError(LightSandboxError):
    """Raised when the server reports an unspecified internal error."""


class LightSandboxConnectionError(LightSandboxError):
    """Raised when the SDK cannot reach the LightSandbox server."""


_CODE_TO_EXCEPTION = {
    "SANDBOX_NOT_FOUND": SandboxNotFound,
    "SANDBOX_EXPIRED": SandboxExpired,
    "EXEC_TIMEOUT": SandboxTimeout,
    "EXEC_FAILED": SandboxExecError,
    "INVALID_PATH": SandboxInvalidPath,
    "FILE_TOO_LARGE": SandboxFileTooLarge,
    "OUTPUT_TOO_LARGE": SandboxOutputTooLarge,
    "RUNTIME_ERROR": SandboxRuntimeError,
    "CONFIG_ERROR": SandboxConfigError,
    "INTERNAL_ERROR": SandboxInternalError,
}


def error_from_response(code: str, message: str) -> LightSandboxError:
    """Maps a server error envelope `{code, message}` to an SDK exception."""
    exc_type = _CODE_TO_EXCEPTION.get(code, LightSandboxError)
    return exc_type(message, code=code)
