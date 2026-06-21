use async_trait::async_trait;

use crate::error::LightSandboxError;
use crate::models::{ExecRequest, ExecResult, SandboxInfo, SandboxSpec};

#[async_trait]
pub trait SandboxRuntime: Send + Sync {
    async fn create(&self, spec: SandboxSpec) -> Result<SandboxInfo, LightSandboxError>;
    async fn list(&self) -> Result<Vec<SandboxInfo>, LightSandboxError>;
    async fn get(&self, id: &str) -> Result<SandboxInfo, LightSandboxError>;
    async fn exec(&self, id: &str, req: ExecRequest) -> Result<ExecResult, LightSandboxError>;
    async fn write_file(
        &self,
        id: &str,
        path: &str,
        content: Vec<u8>,
    ) -> Result<(), LightSandboxError>;
    async fn read_file(&self, id: &str, path: &str) -> Result<Vec<u8>, LightSandboxError>;
    async fn remove(&self, id: &str) -> Result<(), LightSandboxError>;
    async fn cleanup_expired(&self) -> Result<usize, LightSandboxError>;
}
