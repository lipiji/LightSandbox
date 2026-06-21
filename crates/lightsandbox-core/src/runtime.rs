use async_trait::async_trait;
use tokio::sync::mpsc::Sender;

use crate::error::LightSandboxError;
use crate::metrics::MetricsSnapshot;
use crate::models::{ExecOutputEvent, ExecRequest, ExecResult, SandboxInfo, SandboxSpec};

#[async_trait]
pub trait SandboxRuntime: Send + Sync {
    async fn create(&self, spec: SandboxSpec) -> Result<SandboxInfo, LightSandboxError>;
    async fn list(&self) -> Result<Vec<SandboxInfo>, LightSandboxError>;
    async fn get(&self, id: &str) -> Result<SandboxInfo, LightSandboxError>;
    async fn exec(&self, id: &str, req: ExecRequest) -> Result<ExecResult, LightSandboxError>;
    /// Like `exec`, but streams stdout/stderr chunks through `tx` as they
    /// are produced instead of buffering the whole result. Returns `Err`
    /// only for pre-flight failures (e.g. sandbox not found) discovered
    /// before any output has been sent; once the process has started, all
    /// outcomes — including failures — are reported as `ExecOutputEvent`s
    /// terminating in exactly one `Done` or `Error` event.
    async fn exec_stream(
        &self,
        id: &str,
        req: ExecRequest,
        tx: Sender<ExecOutputEvent>,
    ) -> Result<(), LightSandboxError>;
    async fn write_file(
        &self,
        id: &str,
        path: &str,
        content: Vec<u8>,
    ) -> Result<(), LightSandboxError>;
    async fn read_file(&self, id: &str, path: &str) -> Result<Vec<u8>, LightSandboxError>;
    async fn remove(&self, id: &str) -> Result<(), LightSandboxError>;
    async fn cleanup_expired(&self) -> Result<usize, LightSandboxError>;
    /// Returns a point-in-time snapshot of runtime-wide counters and gauges
    /// for observability. Implementations must never fail on a best-effort
    /// read of their own counters; the `Result` keeps the trait uniform.
    async fn metrics(&self) -> Result<MetricsSnapshot, LightSandboxError>;
}
