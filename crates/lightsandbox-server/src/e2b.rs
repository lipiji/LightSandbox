//! Lifecycle-only subset of E2B's control-plane REST API, mapped onto the
//! existing `SandboxRuntime`. Mounted under `/e2b/sandboxes` to mirror E2B's
//! root-level `/sandboxes` path shape without colliding with the native
//! `/v1/sandboxes` namespace. See `docs/e2b-compat.md` for the documented
//! scope and limitations (lifecycle only, no envd exec/filesystem layer).

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use lightsandbox_core::{SandboxInfo, SandboxSpec};
use serde::{Deserialize, Serialize};

use crate::api::ApiError;
use crate::state::AppState;

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/e2b/sandboxes", post(create_sandbox).get(list_sandboxes))
        .route("/e2b/sandboxes/:id", get(get_sandbox).delete(kill_sandbox))
        .route("/e2b/sandboxes/:id/timeout", post(extend_timeout))
        .with_state(state)
}

#[derive(Debug, Default, Deserialize)]
struct E2bCreateRequest {
    #[serde(rename = "templateID")]
    template_id: Option<String>,
    metadata: Option<HashMap<String, String>>,
    #[serde(rename = "envVars")]
    env_vars: Option<HashMap<String, String>>,
    timeout: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct E2bTimeoutRequest {
    timeout: u64,
}

#[derive(Debug, Serialize)]
struct E2bSandbox {
    #[serde(rename = "sandboxID")]
    sandbox_id: String,
    #[serde(rename = "templateID")]
    template_id: Option<String>,
    metadata: HashMap<String, String>,
    #[serde(rename = "startedAt")]
    started_at: DateTime<Utc>,
    #[serde(rename = "endAt")]
    end_at: Option<DateTime<Utc>>,
}

impl E2bSandbox {
    /// `template_id` isn't tracked on `SandboxInfo`, so it's only known at
    /// create time (echoed from the request); list/get always report `null`.
    fn from_info(info: SandboxInfo, template_id: Option<String>) -> Self {
        Self {
            sandbox_id: info.id,
            template_id,
            metadata: info.metadata,
            started_at: info.created_at,
            end_at: info.expires_at,
        }
    }
}

async fn create_sandbox(
    State(state): State<Arc<AppState>>,
    Json(req): Json<E2bCreateRequest>,
) -> Result<(StatusCode, Json<E2bSandbox>), ApiError> {
    let spec = SandboxSpec {
        ttl_seconds: req.timeout,
        metadata: req.metadata,
        env: req.env_vars,
        template: req.template_id.clone(),
    };
    let info = state.runtime.create(spec).await?;
    Ok((
        StatusCode::CREATED,
        Json(E2bSandbox::from_info(info, req.template_id)),
    ))
}

async fn list_sandboxes(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<E2bSandbox>>, ApiError> {
    let infos = state.runtime.list().await?;
    Ok(Json(
        infos
            .into_iter()
            .map(|info| E2bSandbox::from_info(info, None))
            .collect(),
    ))
}

async fn get_sandbox(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<E2bSandbox>, ApiError> {
    let info = state.runtime.get(&id).await?;
    Ok(Json(E2bSandbox::from_info(info, None)))
}

async fn kill_sandbox(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    state.runtime.remove(&id).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn extend_timeout(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<E2bTimeoutRequest>,
) -> Result<Response, ApiError> {
    state.runtime.extend_ttl(&id, req.timeout).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
