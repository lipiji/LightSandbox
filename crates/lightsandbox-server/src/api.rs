use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::{Multipart, Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use futures_util::{Stream, StreamExt};
use lightsandbox_core::{
    format_prometheus, ExecOutputEvent, ExecRequest, FileReadResponse, FileWriteRequest,
    LightSandboxError, SandboxSpec,
};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::state::AppState;

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/metrics", get(metrics))
        .route("/v1/sandboxes", post(create_sandbox).get(list_sandboxes))
        .route("/v1/sandboxes/:id", get(get_sandbox).delete(remove_sandbox))
        .route("/v1/sandboxes/:id/exec", post(exec_sandbox))
        .route("/v1/sandboxes/:id/exec/stream", post(exec_sandbox_stream))
        .route("/v1/sandboxes/:id/files", put(write_file).get(read_file))
        .route("/v1/sandboxes/:id/files/upload", post(upload_file))
        .route("/v1/sandboxes/:id/files/download", get(download_file))
        .with_state(state.clone())
        .merge(crate::e2b::router(state))
}

pub(crate) struct ApiError(LightSandboxError);

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

/// Streams stdout/stderr as Server-Sent Events instead of buffering the
/// whole result. The sandbox is checked up front so a missing/expired
/// sandbox still produces a normal JSON `ApiError` rather than a 200
/// response that immediately errors mid-stream; once the SSE body has
/// started, failures are surfaced in-band as an `error` event.
async fn exec_sandbox_stream(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<ExecRequest>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    state.runtime.get(&id).await?;

    let (tx, rx) = mpsc::channel::<ExecOutputEvent>(16);
    let runtime = state.runtime.clone();
    tokio::spawn(async move {
        let _ = runtime.exec_stream(&id, req, tx).await;
    });

    let stream = ReceiverStream::new(rx).map(|event| Ok(exec_event_to_sse(event)));
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

/// SSE field values may not contain `\r` (axum panics if they do), but
/// process output on Windows is full of `\r\n`. Strip it; `\n` alone is a
/// valid line separator for SSE multi-line data.
fn strip_cr(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).replace('\r', "")
}

fn exec_event_to_sse(event: ExecOutputEvent) -> Event {
    match event {
        ExecOutputEvent::Stdout(chunk) => Event::default().event("stdout").data(strip_cr(&chunk)),
        ExecOutputEvent::Stderr(chunk) => Event::default().event("stderr").data(strip_cr(&chunk)),
        ExecOutputEvent::Done {
            exit_code,
            timed_out,
            duration_ms,
        } => Event::default().event("done").data(
            json!({
                "exit_code": exit_code,
                "timed_out": timed_out,
                "duration_ms": duration_ms,
            })
            .to_string(),
        ),
        ExecOutputEvent::Error(message) => Event::default()
            .event("error")
            .data(message.replace('\r', "")),
    }
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

/// Binary-safe upload via `multipart/form-data`: a "path" text field naming
/// the destination, and a "file" field carrying raw bytes. Unlike the JSON
/// `PUT /files` endpoint (which round-trips content through a UTF-8 string),
/// this preserves arbitrary bytes — needed for non-text files.
async fn upload_file(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut path: Option<String> = None;
    let mut content: Option<Vec<u8>> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError(LightSandboxError::InvalidPath(e.to_string())))?
    {
        match field.name() {
            Some("path") => {
                path = Some(
                    field
                        .text()
                        .await
                        .map_err(|e| ApiError(LightSandboxError::InvalidPath(e.to_string())))?,
                );
            }
            Some("file") => {
                if path.is_none() {
                    path = field.file_name().map(|s| s.to_string());
                }
                content = Some(
                    field
                        .bytes()
                        .await
                        .map_err(|e| ApiError(LightSandboxError::InvalidPath(e.to_string())))?
                        .to_vec(),
                );
            }
            _ => {}
        }
    }

    let path = path.ok_or_else(|| {
        ApiError(LightSandboxError::InvalidPath(
            "missing \"path\" field and no filename on \"file\" field".to_string(),
        ))
    })?;
    let content = content.ok_or_else(|| {
        ApiError(LightSandboxError::InvalidPath(
            "missing \"file\" field".to_string(),
        ))
    })?;

    state.runtime.write_file(&id, &path, content).await?;
    Ok(Json(json!({"written": true, "path": path})))
}

/// Binary-safe download: returns the raw file bytes with
/// `application/octet-stream` instead of wrapping them in a JSON string
/// (which would require lossy UTF-8 conversion for non-text files).
async fn download_file(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<ReadFileQuery>,
) -> Result<Response, ApiError> {
    let bytes = state.runtime.read_file(&id, &query.path).await?;
    let filename = query
        .path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(&query.path)
        .to_string();
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/octet-stream".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
        ],
        bytes,
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exercises the `ApiError` -> HTTP status mapping exhaustively. This is
    /// the REST API's error contract; only `SandboxNotFound -> 404` is
    /// reachable through the existing HTTP-level tests, so the rest of the
    /// matrix (some of which is awkward to trigger via endpoints — e.g.
    /// `SandboxExpired` needs a timed-out sandbox, `OutputTooLarge` needs a
    /// custom read cap) is pinned here at the mapping itself.
    #[test]
    fn error_variants_map_to_documented_status_codes() {
        fn status_of(err: LightSandboxError) -> StatusCode {
            ApiError(err).into_response().status()
        }

        assert_eq!(
            status_of(LightSandboxError::SandboxNotFound),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            status_of(LightSandboxError::SandboxExpired),
            StatusCode::GONE
        );
        assert_eq!(
            status_of(LightSandboxError::InvalidPath("x".into())),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            status_of(LightSandboxError::ExecTimeout),
            StatusCode::REQUEST_TIMEOUT
        );
        assert_eq!(
            status_of(LightSandboxError::ExecFailed("boom".into())),
            StatusCode::BAD_GATEWAY
        );
        // Both size-limit errors collapse to 413.
        assert_eq!(
            status_of(LightSandboxError::FileTooLarge),
            StatusCode::PAYLOAD_TOO_LARGE
        );
        assert_eq!(
            status_of(LightSandboxError::OutputTooLarge),
            StatusCode::PAYLOAD_TOO_LARGE
        );
        assert_eq!(
            status_of(LightSandboxError::RuntimeError("oops".into())),
            StatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!(
            status_of(LightSandboxError::ConfigError("bad".into())),
            StatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!(
            status_of(LightSandboxError::InternalError),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    /// Every error response carries the stable `{"error":{"code","message"}}`
    /// envelope regardless of variant — a sample check here complements the
    /// full status matrix above and guards the body shape at the mapping layer.
    #[tokio::test]
    async fn error_response_carries_stable_envelope() {
        let response = ApiError(LightSandboxError::FileTooLarge).into_response();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["error"]["code"], "FILE_TOO_LARGE");
        assert!(value["error"]["message"].is_string());
    }
}
