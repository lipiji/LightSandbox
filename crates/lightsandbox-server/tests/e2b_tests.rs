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

fn test_config(workspace_root: PathBuf) -> RuntimeConfig {
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
        templates_dir: None,
        pool_enabled: false,
        pool_min_idle: 0,
        persistence_db_path: None,
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
    let runtime = Arc::new(
        LocalProcessRuntime::new(test_config(unique_dir("e2b_test"))).expect("test runtime builds"),
    );
    let state = Arc::new(AppState { runtime });
    api::router(state)
}

async fn body_json(response: axum::response::Response) -> (StatusCode, Value) {
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, value)
}

#[tokio::test]
async fn create_list_get_round_trip() {
    let app = test_app();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/e2b/sandboxes")
                .header("content-type", "application/json")
                .body(Body::from(json!({"timeout": 120}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let (status, value) = body_json(response).await;
    assert_eq!(status, StatusCode::CREATED);
    assert!(value["sandboxID"].as_str().unwrap().starts_with("sbx_"));
    assert!(value["templateID"].is_null());
    assert!(value["startedAt"].is_string());
    let id = value["sandboxID"].as_str().unwrap().to_string();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/e2b/sandboxes")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let (status, value) = body_json(response).await;
    assert_eq!(status, StatusCode::OK);
    assert!(value
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["sandboxID"] == id));

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/e2b/sandboxes/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let (status, value) = body_json(response).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["sandboxID"], id);
}

#[tokio::test]
async fn extend_timeout_moves_native_expiry_forward() {
    let app = test_app();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/e2b/sandboxes")
                .header("content-type", "application/json")
                .body(Body::from(json!({"timeout": 10}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let (_, value) = body_json(response).await;
    let id = value["sandboxID"].as_str().unwrap().to_string();
    let original_expiry = value["endAt"].as_str().unwrap().to_string();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/e2b/sandboxes/{id}/timeout"))
                .header("content-type", "application/json")
                .body(Body::from(json!({"timeout": 9999}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/v1/sandboxes/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let (_, value) = body_json(response).await;
    let new_expiry = value["expires_at"].as_str().unwrap().to_string();
    assert!(new_expiry > original_expiry);
}

#[tokio::test]
async fn kill_removes_sandbox_from_native_api() {
    let app = test_app();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/e2b/sandboxes")
                .header("content-type", "application/json")
                .body(Body::from(json!({}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let (_, value) = body_json(response).await;
    let id = value["sandboxID"].as_str().unwrap().to_string();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/e2b/sandboxes/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

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
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(value["error"]["code"], "SANDBOX_NOT_FOUND");
}

#[tokio::test]
async fn create_with_unknown_template_returns_standard_error_envelope() {
    let app = test_app();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/e2b/sandboxes")
                .header("content-type", "application/json")
                .body(Body::from(json!({"templateID": "missing"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let (status, value) = body_json(response).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(value["error"]["code"], "INVALID_PATH");
}
