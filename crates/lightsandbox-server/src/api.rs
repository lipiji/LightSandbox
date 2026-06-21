use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use lightsandbox_core::{
    format_prometheus, ExecRequest, FileReadResponse, FileWriteRequest, LightSandboxError,
    SandboxSpec,
};
use serde::Deserialize;
use serde_json::json;

use crate::state::AppState;

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/metrics", get(metrics))
        .route("/v1/sandboxes", post(create_sandbox).get(list_sandboxes))
        .route("/v1/sandboxes/:id", get(get_sandbox).delete(remove_sandbox))
        .route("/v1/sandboxes/:id/exec", post(exec_sandbox))
        .route("/v1/sandboxes/:id/files", put(write_file).get(read_file))
        .with_state(state)
}

struct ApiError(LightSandboxError);

impl From<LightSandboxError> for ApiError {
    fn from(e: LightSandboxError) -> Self {
        ApiError(e)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match &self.0 {
            LightSandboxError::SandboxNotFound => StatusCode::NOT_FOUND,
            LightSandboxError::SandboxExpired => StatusCode::GONE,
            LightSandboxError::InvalidPath(_) => StatusCode::BAD_REQUEST,
            LightSandboxError::ExecTimeout => StatusCode::REQUEST_TIMEOUT,
            LightSandboxError::ExecFailed(_) => StatusCode::BAD_GATEWAY,
            LightSandboxError::FileTooLarge | LightSandboxError::OutputTooLarge => {
                StatusCode::PAYLOAD_TOO_LARGE
            }
            LightSandboxError::RuntimeError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            LightSandboxError::ConfigError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            LightSandboxError::InternalError => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (status, Json(self.0.to_response())).into_response()
    }
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({"status": "ok"}))
}

/// Prometheus exposition endpoint. Returns the runtime's metrics snapshot
/// formatted as a 0.0.4 text exposition with the standard content type so a
/// scrape picks it up without extra configuration.
async fn metrics(State(state): State<Arc<AppState>>) -> Result<Response, ApiError> {
    let snap = state.runtime.metrics().await?;
    let body = format_prometheus(&snap);
    Ok((
        StatusCode::OK,
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        body,
    )
        .into_response())
}

async fn create_sandbox(
    State(state): State<Arc<AppState>>,
    Json(spec): Json<SandboxSpec>,
) -> Result<Json<lightsandbox_core::SandboxInfo>, ApiError> {
    let info = state.runtime.create(spec).await?;
    Ok(Json(info))
}

async fn list_sandboxes(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<lightsandbox_core::SandboxInfo>>, ApiError> {
    let infos = state.runtime.list().await?;
    Ok(Json(infos))
}

async fn get_sandbox(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<lightsandbox_core::SandboxInfo>, ApiError> {
    let info = state.runtime.get(&id).await?;
    Ok(Json(info))
}

async fn remove_sandbox(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    state.runtime.remove(&id).await?;
    Ok(Json(json!({"removed": true})))
}

async fn exec_sandbox(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<ExecRequest>,
) -> Result<Json<lightsandbox_core::ExecResult>, ApiError> {
    let result = state.runtime.exec(&id, req).await?;
    Ok(Json(result))
}

async fn write_file(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<FileWriteRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    state
        .runtime
        .write_file(&id, &req.path, req.content.into_bytes())
        .await?;
    Ok(Json(json!({"written": true})))
}

#[derive(Debug, Deserialize)]
struct ReadFileQuery {
    path: String,
}

async fn read_file(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<ReadFileQuery>,
) -> Result<Json<FileReadResponse>, ApiError> {
    let bytes = state.runtime.read_file(&id, &query.path).await?;
    Ok(Json(FileReadResponse {
        path: query.path,
        content: String::from_utf8_lossy(&bytes).into_owned(),
    }))
}
