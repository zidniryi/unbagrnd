//! Integration tests against the real `axum::Router` (via `tower::oneshot`,
//! no actual TCP socket) and a throwaway temp-file SQLite database. None of
//! these touch the background-removal model itself - that's the one path
//! gated behind `UNBAGRND_TEST_MODEL_*`, mirroring the same convention
//! `unbagrnd-core`'s own test uses, so `cargo test` stays fast and offline
//! by default.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use tower::ServiceExt;

use unbagrnd_api::config::Config;
use unbagrnd_api::state::AppState;
use unbagrnd_api::{db, routes};

const TEST_ADMIN_KEY: &str = "test-admin-key";

async fn test_state() -> Arc<AppState> {
    let db_path = std::env::temp_dir().join(format!("unbagrnd-test-{}.db", uuid::Uuid::new_v4()));
    let database_url = format!("sqlite://{}", db_path.display());
    let pool = db::connect(&database_url).await.expect("test db connect");

    let spec = unbagrnd_core::models::find_model(unbagrnd_core::models::DEFAULT_MODEL_KEY)
        .expect("default model is always in the catalog");

    let config = Config {
        host: "127.0.0.1".to_string(),
        port: 0,
        database_url,
        models_dir: std::env::temp_dir(),
        model_key: spec.key.to_string(),
        admin_key: Some(TEST_ADMIN_KEY.to_string()),
        max_file_size_mb: 20,
        max_image_width: 8192,
        max_image_height: 8192,
        max_concurrent_inferences: 2,
        max_batch_size: 3,
        rate_limit_per_minute: 60,
        cors_origins: "*".to_string(),
    };

    Arc::new(AppState {
        db: pool,
        inference: unbagrnd_core::bg_remove::InferenceState::new(),
        model_spec: spec,
        // Never actually read in these tests - no test here reaches the
        // inference call.
        model_path: PathBuf::from("/nonexistent/model.onnx"),
        config,
        inference_semaphore: Arc::new(tokio::sync::Semaphore::new(2)),
        rate_limiter: Mutex::new(HashMap::new()),
    })
}

fn app(state: Arc<AppState>) -> Router {
    routes::build_router(state)
}

async fn json_body(response: axum::response::Response) -> serde_json::Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).expect("response body should be JSON")
}

async fn create_key(state: &Arc<AppState>) -> (String, String) {
    let response = app(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/keys")
                .header("X-Admin-Key", TEST_ADMIN_KEY)
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    (
        body["api_key"].as_str().unwrap().to_string(),
        body["id"].as_str().unwrap().to_string(),
    )
}

#[tokio::test]
async fn health_requires_no_auth() {
    let state = test_state().await;
    let response = app(state)
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["status"], "ok");
}

#[tokio::test]
async fn remove_background_without_key_is_unauthorized() {
    let state = test_state().await;
    let response = app(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/remove-background")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn remove_background_with_unknown_key_is_unauthorized() {
    let state = test_state().await;
    let response = app(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/remove-background")
                .header("X-API-Key", "unb_live_00000000000000000000000000000000")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn keys_routes_require_admin_key() {
    let state = test_state().await;
    let response = app(state)
        .oneshot(
            Request::builder()
                .uri("/v1/keys")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_generated_key_is_returned_once_and_never_stored_in_plaintext() {
    let state = test_state().await;
    let (api_key, id) = create_key(&state).await;
    assert!(api_key.starts_with("unb_live_"));

    let records = unbagrnd_api::db::list_api_keys(&state.db).await.unwrap();
    let record = records.into_iter().find(|r| r.id == id).unwrap();
    assert_ne!(
        record.key_hash, api_key,
        "the plaintext key must never be stored"
    );
    assert_eq!(record.key_hash.len(), 64, "sha256 hex digest is 64 chars");
}

#[tokio::test]
async fn revoked_key_immediately_stops_working() {
    let state = test_state().await;
    let (api_key, id) = create_key(&state).await;

    let response = app(state.clone())
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/v1/keys/{id}"))
                .header("X-Admin-Key", TEST_ADMIN_KEY)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    // Revoking again (or an unknown id) is a 404, not a second success.
    let response = app(state.clone())
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/v1/keys/{id}"))
                .header("X-Admin-Key", TEST_ADMIN_KEY)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let response = app(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/remove-background")
                .header("X-API-Key", &api_key)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn remove_background_rejects_a_non_image_upload() {
    let state = test_state().await;
    let (api_key, _id) = create_key(&state).await;

    let boundary = "X-TEST-BOUNDARY";
    let body = format!(
        "--{boundary}\r\n\
         Content-Disposition: form-data; name=\"image\"; filename=\"not-an-image.txt\"\r\n\
         Content-Type: text/plain\r\n\r\n\
         this is not an image\r\n\
         --{boundary}--\r\n"
    );

    let response = app(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/remove-background")
                .header("X-API-Key", &api_key)
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = json_body(response).await;
    assert_eq!(body["error"]["code"], "UNSUPPORTED_IMAGE");
}

#[tokio::test]
async fn remove_background_rejects_a_request_with_no_image_field() {
    let state = test_state().await;
    let (api_key, _id) = create_key(&state).await;

    let boundary = "X-TEST-BOUNDARY";
    let body = format!(
        "--{boundary}\r\n\
         Content-Disposition: form-data; name=\"not_image\"\r\n\r\n\
         hello\r\n\
         --{boundary}--\r\n"
    );

    let response = app(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/remove-background")
                .header("X-API-Key", &api_key)
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = json_body(response).await;
    assert_eq!(body["error"]["code"], "INVALID_IMAGE");
}

#[tokio::test]
async fn remove_background_batch_without_key_is_unauthorized() {
    let state = test_state().await;
    let response = app(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/remove-background/batch")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn remove_background_batch_rejects_a_request_with_no_images_field() {
    let state = test_state().await;
    let (api_key, _id) = create_key(&state).await;

    let boundary = "X-TEST-BOUNDARY";
    let body = format!(
        "--{boundary}\r\n\
         Content-Disposition: form-data; name=\"not_images\"\r\n\r\n\
         hello\r\n\
         --{boundary}--\r\n"
    );

    let response = app(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/remove-background/batch")
                .header("X-API-Key", &api_key)
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = json_body(response).await;
    assert_eq!(body["error"]["code"], "NO_IMAGES");
}

/// A batch that mixes a valid field name with an unreadable file doesn't
/// fail the whole request - the bad file shows up as a per-item error in an
/// otherwise-200 response, so the caller still gets back everything that
/// *did* work.
#[tokio::test]
async fn remove_background_batch_reports_a_non_image_file_as_a_per_item_error() {
    let state = test_state().await;
    let (api_key, _id) = create_key(&state).await;

    let boundary = "X-TEST-BOUNDARY";
    let body = format!(
        "--{boundary}\r\n\
         Content-Disposition: form-data; name=\"images\"; filename=\"not-an-image.txt\"\r\n\
         Content-Type: text/plain\r\n\r\n\
         this is not an image\r\n\
         --{boundary}--\r\n"
    );

    let response = app(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/remove-background/batch")
                .header("X-API-Key", &api_key)
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    let results = body["results"].as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["filename"], "not-an-image.txt");
    assert_eq!(results[0]["status"], "error");
    assert_eq!(results[0]["error"]["code"], "UNSUPPORTED_IMAGE");
}

/// `test_state()` sets `max_batch_size` to 3 - a 4th `images` field should
/// be rejected outright rather than silently dropped or processed anyway.
#[tokio::test]
async fn remove_background_batch_rejects_more_files_than_the_batch_limit() {
    let state = test_state().await;
    let (api_key, _id) = create_key(&state).await;

    let boundary = "X-TEST-BOUNDARY";
    let mut body = String::new();
    for i in 0..4 {
        body.push_str(&format!(
            "--{boundary}\r\n\
             Content-Disposition: form-data; name=\"images\"; filename=\"file{i}.txt\"\r\n\
             Content-Type: text/plain\r\n\r\n\
             not an image {i}\r\n"
        ));
    }
    body.push_str(&format!("--{boundary}--\r\n"));

    let response = app(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/remove-background/batch")
                .header("X-API-Key", &api_key)
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = json_body(response).await;
    assert_eq!(body["error"]["code"], "BATCH_TOO_LARGE");
}
