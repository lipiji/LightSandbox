pub mod api;
pub mod config;
pub mod gc;
pub mod state;

use std::path::Path;
use std::sync::Arc;

use lightsandbox_core::LightSandboxError;
use lightsandbox_runtime_local::LocalProcessRuntime;

use crate::config::AppConfig;
use crate::state::AppState;

/// Loads config, builds the runtime, starts the GC task, and serves the
/// REST API until the process is killed or the listener fails.
pub async fn run(config_path: &Path) -> Result<(), LightSandboxError> {
    let app_config = AppConfig::load(config_path)?;
    let runtime = Arc::new(LocalProcessRuntime::new(app_config.runtime_config()));
    let state = Arc::new(AppState {
        runtime: runtime.clone(),
    });

    gc::spawn(
        state.clone(),
        app_config.gc.interval_seconds,
        app_config.gc.enabled,
    );

    let addr = app_config.socket_addr()?;
    tracing::info!(%addr, "starting lightsandbox-server");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| LightSandboxError::RuntimeError(format!("failed to bind {addr}: {e}")))?;

    axum::serve(listener, api::router(state))
        .await
        .map_err(|e| LightSandboxError::RuntimeError(e.to_string()))
}
