use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum LightSandboxError {
    #[error("sandbox not found")]
    SandboxNotFound,
    #[error("sandbox expired")]
    SandboxExpired,
    #[error("invalid path: {0}")]
    InvalidPath(String),
    #[error("execution timed out")]
    ExecTimeout,
    #[error("execution failed: {0}")]
    ExecFailed(String),
    #[error("file too large")]
    FileTooLarge,
    #[error("output too large")]
    OutputTooLarge,
    #[error("runtime error: {0}")]
    RuntimeError(String),
    #[error("config error: {0}")]
    ConfigError(String),
    #[error("internal error")]
    InternalError,
}

impl LightSandboxError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::SandboxNotFound => "SANDBOX_NOT_FOUND",
            Self::SandboxExpired => "SANDBOX_EXPIRED",
            Self::InvalidPath(_) => "INVALID_PATH",
            Self::ExecTimeout => "EXEC_TIMEOUT",
            Self::ExecFailed(_) => "EXEC_FAILED",
            Self::FileTooLarge => "FILE_TOO_LARGE",
            Self::OutputTooLarge => "OUTPUT_TOO_LARGE",
            Self::RuntimeError(_) => "RUNTIME_ERROR",
            Self::ConfigError(_) => "CONFIG_ERROR",
            Self::InternalError => "INTERNAL_ERROR",
        }
    }

    /// User-facing message. Never includes raw panics or host filesystem paths.
    pub fn message(&self) -> String {
        self.to_string()
    }

    pub fn to_response(&self) -> ErrorResponse {
        ErrorResponse {
            error: ErrorBody {
                code: self.code().to_string(),
                message: self.message(),
            },
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: ErrorBody,
}

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
}
