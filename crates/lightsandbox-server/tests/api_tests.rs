use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use lightsandbox_core::{ResourceLimits, RuntimeConfig};
use lightsandbox_runtime_local::LocalProcessRuntime;
use lightsandbox_server::{api, state::AppState};
use serde_json::{json, Value};
use tower::ServiceExt;

fn test_app() -> axum::Router {
    let dir = std::env::temp_dir().join(format!(
        "lightsandbox_api_test_{:x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let config = RuntimeConfig {
        workspace_root: dir,
        limits: ResourceLimits {
            max_sandboxes: 100,
            max_concurrent_exec: 20,
            default_ttl_seconds: 600,
            default_exec_timeout_seconds: 30,
            max_stdout_bytes: 4096,
            max_stderr_bytes: 4096,
            max_file_size_bytes: 1024,
            max_read_file_bytes: 1024,
        },
        allow_absolute_paths: false,
        allow_path_traversal: false,
        hide_host_paths: true,
        remove_expired: true,
    };
    let runtime = Arc::new(LocalProcessRuntime::new(config));
    let state = Arc::new(AppState { runtime });
    api::router(state)
}

async fn body_json(response: axum::response::Response) -> (StatusCode, Value) {
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    (status, value)
}

/// Collects the raw response body as text — used for the `/metrics` endpoint,
/// which is Prometheus text format rather than JSON.
async fn body_text(response: axum::response::Response) -> (StatusCode, String) {
    let status = response.status();
    let ct = response
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        format!("{}|{}", ct, String::from_utf8_lossy(&bytes)),
    )
}

#[tokio::test]
async fn health_check_ok() {
    let app = test_app();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let (status, value) = body_json(response).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value, json!({"status": "ok"}));
}

#[tokio::test]
async fn create_then_get_sandbox() {
    let app = test_app();
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/sandboxes")
                .header("content-type", "application/json")
                .body(Body::from(json!({"ttl_seconds": 60}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let (status, value) = body_json(response).await;
    assert_eq!(status, StatusCode::OK);
    let id = value["id"].as_str().unwrap().to_string();
    assert!(id.starts_with("sbx_"));

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/v1/sandboxes/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let (status, value) = body_json(response).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["id"], id);
}

#[tokio::test]
async fn get_unknown_sandbox_returns_stable_error_envelope() {
    let app = test_app();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/sandboxes/sbx_does_not_exist")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let (status, value) = body_json(response).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(value["error"]["code"], "SANDBOX_NOT_FOUND");
    assert!(value["error"]["message"].is_string());
}

#[tokio::test]
async fn write_read_remove_round_trip() {
    let app = test_app();
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/sandboxes")
                .header("content-type", "application/json")
                .body(Body::from(json!({}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let (_, value) = body_json(response).await;
    let id = value["id"].as_str().unwrap().to_string();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/v1/sandboxes/{id}/files"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"path": "main.py", "content": "print('hi')"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let (status, _) = body_json(response).await;
    assert_eq!(status, StatusCode::OK);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/sandboxes/{id}/files?path=main.py"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let (status, value) = body_json(response).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["content"], "print('hi')");

    let response = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/v1/sandboxes/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let (status, value) = body_json(response).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["removed"], true);
}

#[tokio::test]
async fn path_traversal_returns_invalid_path_error() {
    let app = test_app();
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/sandboxes")
                .header("content-type", "application/json")
                .body(Body::from(json!({}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let (_, value) = body_json(response).await;
    let id = value["id"].as_str().unwrap().to_string();

    let response = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/v1/sandboxes/{id}/files"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"path": "../escape.txt", "content": "x"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let (status, value) = body_json(response).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(value["error"]["code"], "INVALID_PATH");
}

#[tokio::test]
async fn metrics_endpoint_exposes_prometheus_text() {
    let app = test_app();

    // Drive some traffic so the counters are non-zero and exercise the
    // histogram path.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/sandboxes")
                .header("content-type", "application/json")
                .body(Body::from(json!({}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let (_, value) = body_json(response).await;
    let id = value["id"].as_str().unwrap().to_string();
    let _ = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/sandboxes/{id}/exec"))
                .header("content-type", "application/json")
                .body(Body::from(json!({"cmd": "echo metrics"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let (status, ct_and_body) = body_text(response).await;
    assert_eq!(status, StatusCode::OK);
    let (content_type, body) = ct_and_body.split_once('|').unwrap();

    // Prometheus 0.0.4 exposition content type.
    assert!(
        content_type.starts_with("text/plain"),
        "got: {content_type}"
    );
    assert!(body.contains("# TYPE lightsandbox_sandboxes_created_total counter"));
    assert!(body.contains("# TYPE lightsandbox_sandboxes_active gauge"));
    assert!(body.contains("# TYPE lightsandbox_exec_duration_seconds histogram"));
    assert!(
        body.contains("lightsandbox_exec_duration_seconds_bucket{le=\"+Inf\"}"),
        "missing +Inf bucket line"
    );
    // A created sandbox should be reflected in the counter.
    assert!(body.contains("lightsandbox_sandboxes_created_total 1"));
}
