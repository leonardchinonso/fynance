//! Smoke tests for the Phase 2 Axum router: build the app against a
//! tempfile DB and drive it in-process with `tower::ServiceExt::oneshot`
//! so we exercise middleware + handlers without binding a socket.

use std::sync::{Arc, Mutex};

use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use fynance::server::build_router;
use fynance::storage::Db;
use tempfile::tempdir;
use tower::ServiceExt;

fn test_router() -> (axum::Router, Arc<Mutex<Db>>) {
    let dir = tempdir().unwrap();
    // Leak the tempdir so it outlives the test process -- the path is
    // /tmp-ish and OS cleanup will handle it.
    let path = dir.keep().join("server_smoke.db");
    let db = Db::open(&path).unwrap();
    let shared = Arc::new(Mutex::new(db));
    (build_router(shared.clone(), true), shared)
}

fn request(method: Method, uri: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::empty())
        .unwrap()
}

fn request_json(method: Method, uri: &str, body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap()
}

#[tokio::test]
async fn health_endpoint_responds_without_auth() {
    let (app, _) = test_router();
    let response = app
        .oneshot(request(Method::GET, "/api/health"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["status"], "ok");
}

#[tokio::test]
async fn openapi_docs_endpoint_embeds_categories() {
    let (app, _) = test_router();
    let response = app
        .oneshot(request(Method::GET, "/api/docs"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let spec: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(spec["openapi"], "3.1.0");
    assert!(spec["x-fynance"]["categories"].is_object());
    assert!(spec["paths"]["/api/transactions"].is_object());
}

#[tokio::test]
async fn unknown_path_falls_back_to_embedded_index_html() {
    let (app, _) = test_router();
    let response = app
        .oneshot(request(Method::GET, "/some/spa/route"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(content_type.starts_with("text/html"));
}

#[tokio::test]
async fn fresh_db_has_no_default_profile() {
    let (app, _) = test_router();
    let response = app
        .oneshot(request(Method::GET, "/api/profiles"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let profiles: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        profiles.as_array().map(|a| a.len()),
        Some(0),
        "a fresh database must not auto-seed a default profile"
    );
}

fn new_account_body(profile_ids: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "id": "acc1",
        "name": "Current",
        "institution": "TestBank",
        "type": "checking",
        "profile_ids": profile_ids,
    })
}

#[tokio::test]
async fn create_account_requires_a_profile() {
    let (app, _) = test_router();
    let response = app
        .oneshot(request_json(
            Method::POST,
            "/api/accounts",
            new_account_body(serde_json::json!([])),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn create_account_rejects_unknown_profile() {
    let (app, _) = test_router();
    let response = app
        .oneshot(request_json(
            Method::POST,
            "/api/accounts",
            new_account_body(serde_json::json!(["ghost"])),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn create_account_succeeds_with_existing_profile() {
    let (app, db) = test_router();
    {
        let db = db.lock().unwrap();
        db.create_profile("alice", "Alice").unwrap();
    }
    let response = app
        .oneshot(request_json(
            Method::POST,
            "/api/accounts",
            new_account_body(serde_json::json!(["alice"])),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let account: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(account["profile_ids"], serde_json::json!(["alice"]));
}

#[tokio::test]
async fn token_lifecycle_create_validate_revoke() {
    let (_app, db) = test_router();
    let raw = {
        let db = db.lock().unwrap();
        db.create_token("smoke").unwrap()
    };
    assert!(raw.starts_with("fyn_"));

    {
        let db = db.lock().unwrap();
        let name = db.validate_token(&raw).unwrap();
        assert_eq!(name.as_deref(), Some("smoke"));
    }

    {
        let db = db.lock().unwrap();
        db.revoke_token("smoke").unwrap();
        let after = db.validate_token(&raw).unwrap();
        assert!(after.is_none());
    }
}
