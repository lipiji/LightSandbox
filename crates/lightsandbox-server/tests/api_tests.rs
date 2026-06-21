use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use lightsandbox_core::{ResourceLimits, RuntimeConfig};
use lightsandbox_runtime_local::LocalProcessRuntime;
use lightsandbox_server::{api, state::AppState};
use serde_json::{json, Value};
use tower::ServiceExt;

fn test_config(workspace_root: PathBuf, templates_dir: Option<PathBuf>) -> RuntimeConfig {
    RuntimeConfig {
        workspace_root,
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
        templates_dir,
        pool_enabled: false,
        pool_min_idle: 0,
    }
}

fn unique_dir(prefix: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "lightsandbox_{prefix}_{:x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn test_app() -> axum::Router {
    let runtime = Arc::new(LocalProcessRuntime::new(test_config(
        unique_dir("api_test"),
        None,
    )));
    let state = Arc::new(AppState { runtime });
    api::router(state)
}

fn test_app_with_templates(templates_dir: PathBuf) -> axum::Router {
    let runtime = Arc::new(LocalProcessRuntime::new(test_config(
        unique_dir("api_tpl"),
        Some(templates_dir),
    )));
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

#[tokio::test]
async fn exec_stream_returns_sse_events() {
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
                .method("POST")
                .uri(format!("/v1/sandboxes/{id}/exec/stream"))
                .header("content-type", "application/json")
                .body(Body::from(json!({"cmd": "echo stream-test"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let content_type = response
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert!(
        content_type.starts_with("text/event-stream"),
        "got: {content_type}"
    );

    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8_lossy(&bytes);

    assert!(body.contains("event: stdout"), "body: {body}");
    assert!(body.contains("stream-test"), "body: {body}");
    assert!(body.contains("event: done"), "body: {body}");
    assert!(body.contains("\"exit_code\":0"), "body: {body}");
}

#[tokio::test]
async fn exec_stream_unknown_sandbox_returns_json_error() {
    let app = test_app();
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/sandboxes/sbx_does_not_exist/exec/stream")
                .header("content-type", "application/json")
                .body(Body::from(json!({"cmd": "echo hi"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let (status, value) = body_json(response).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(value["error"]["code"], "SANDBOX_NOT_FOUND");
}

#[tokio::test]
async fn create_with_template_populates_workspace_via_api() {
    let templates_root = unique_dir("templates");
    let tpl = templates_root.join("hello");
    std::fs::create_dir_all(&tpl).unwrap();
    std::fs::write(tpl.join("seed.txt"), "templated content").unwrap();

    let app = test_app_with_templates(templates_root);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/sandboxes")
                .header("content-type", "application/json")
                .body(Body::from(json!({"template": "hello"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let (status, value) = body_json(response).await;
    assert_eq!(status, StatusCode::OK);
    let id = value["id"].as_str().unwrap().to_string();

    // The templated file should be readable without any prior write_file.
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/v1/sandboxes/{id}/files?path=seed.txt"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let (status, value) = body_json(response).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["content"], "templated content");
}

fn multipart_body(boundary: &str, path: &str, filename: &str, content: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(b"Content-Disposition: form-data; name=\"path\"\r\n\r\n");
    body.extend_from_slice(path.as_bytes());
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        format!(
            "Content-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(content);
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    body
}

#[tokio::test]
async fn upload_download_round_trip_is_binary_safe() {
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

    // Includes bytes that are not valid UTF-8, which the JSON-based
    // `/files` endpoint cannot round-trip losslessly.
    let binary_content: Vec<u8> = vec![0, 159, 146, 150, 255, 0, 1, 2];
    let boundary = "lightsandboxtestboundary";
    let body = multipart_body(boundary, "binary.dat", "binary.dat", &binary_content);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/sandboxes/{id}/files/upload"))
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    let (status, value) = body_json(response).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["path"], "binary.dat");

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/v1/sandboxes/{id}/files/download?path=binary.dat"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let content_type = response
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert_eq!(content_type, "application/octet-stream");
    let disposition = response
        .headers()
        .get(axum::http::header::CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert!(disposition.contains("binary.dat"), "got: {disposition}");
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(bytes.to_vec(), binary_content);
}

#[tokio::test]
async fn upload_path_traversal_returns_invalid_path_error() {
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

    let boundary = "lightsandboxtestboundary2";
    let body = multipart_body(boundary, "../escape.dat", "escape.dat", b"x");

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/sandboxes/{id}/files/upload"))
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    let (status, value) = body_json(response).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(value["error"]["code"], "INVALID_PATH");
}

#[tokio::test]
async fn create_with_unknown_template_returns_invalid_path() {
    let app = test_app_with_templates(unique_dir("templates_empty"));
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/sandboxes")
                .header("content-type", "application/json")
                .body(Body::from(json!({"template": "missing"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let (status, value) = body_json(response).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(value["error"]["code"], "INVALID_PATH");
}
