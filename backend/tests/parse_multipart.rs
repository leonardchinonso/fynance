//! Integration tests for `POST /api/parse` multipart handling.
//!
//! These tests build a real `multipart/form-data` body with N files (small
//! CSVs) and drive the router via `tower::ServiceExt::oneshot`. They
//! exercise everything up to the LLM call: multipart parsing, hint
//! deserialization, file preprocessing, account lookup. The actual LLM
//! call won't be reached (no API key configured in tests, and the test
//! account doesn't exist), which is the point — we want a fast,
//! deterministic check that the upload path doesn't choke on multiple
//! files.

use std::sync::{Arc, Mutex};

use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use fynance::server::build_router;
use fynance::storage::Db;
use tempfile::tempdir;
use tower::ServiceExt;

const BOUNDARY: &str = "----TestBoundary123";

fn test_router() -> (axum::Router, Arc<Mutex<Db>>) {
    let dir = tempdir().unwrap();
    let path = dir.keep().join("parse_multipart.db");
    let db = Db::open(&path).unwrap();
    let shared = Arc::new(Mutex::new(db));
    (build_router(shared.clone(), true), shared)
}

enum Part<'a> {
    File {
        name: &'a str,
        filename: &'a str,
        content_type: &'a str,
        body: &'a [u8],
    },
    Text {
        name: &'a str,
        body: &'a str,
    },
}

fn multipart_body(parts: &[Part<'_>]) -> Vec<u8> {
    let mut out = Vec::new();
    for part in parts {
        out.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());
        match part {
            Part::File { name, filename, content_type, body } => {
                out.extend_from_slice(
                    format!("Content-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\"\r\n")
                        .as_bytes(),
                );
                out.extend_from_slice(format!("Content-Type: {content_type}\r\n\r\n").as_bytes());
                out.extend_from_slice(body);
                out.extend_from_slice(b"\r\n");
            }
            Part::Text { name, body } => {
                out.extend_from_slice(
                    format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
                );
                out.extend_from_slice(body.as_bytes());
                out.extend_from_slice(b"\r\n");
            }
        }
    }
    out.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    out
}

const HINTS: &str = r#"{"return_type":{"transactions":true,"holdings":{"enabled":false,"period":null},"investments":false},"experimental":null,"hint":null}"#;

const TINY_CSV: &[u8] = b"date,description,amount\n2026-05-01,Test,-1.00\n";

async fn post_parse(app: axum::Router, body: Vec<u8>) -> (StatusCode, serde_json::Value) {
    let req = Request::builder()
        .method(Method::POST)
        .uri("/api/parse")
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .body(Body::from(body))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value =
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

fn make_parts<'a>(filenames: &'a [&'a str]) -> Vec<Part<'a>> {
    let mut parts: Vec<Part> = filenames
        .iter()
        .map(|f| Part::File {
            name: "files[]",
            filename: f,
            content_type: "text/csv",
            body: TINY_CSV,
        })
        .collect();
    parts.push(Part::Text {
        name: "account_id",
        body: "missing-account",
    });
    parts.push(Part::Text {
        name: "hints",
        body: HINTS,
    });
    parts
}

/// Baseline: single file gets past multipart + validation. Account doesn't
/// exist in the test DB, so we expect a 400 `account_not_found` — that's the
/// signal that the upload path actually succeeded.
#[tokio::test]
async fn single_file_reaches_account_lookup() {
    let (app, _) = test_router();
    let parts = make_parts(&["one.csv"]);
    let (status, json) = post_parse(app, multipart_body(&parts)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {json}");
    assert_eq!(json["code"], "account_not_found", "body: {json}");
}

/// The regression case the user hit in the browser: two files reach the
/// handler successfully too. If the multipart loop ever chokes on a second
/// `files[]` part, this test fails fast.
#[tokio::test]
async fn two_files_reach_account_lookup() {
    let (app, _) = test_router();
    let parts = make_parts(&["one.csv", "two.csv"]);
    let (status, json) = post_parse(app, multipart_body(&parts)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {json}");
    assert_eq!(json["code"], "account_not_found", "body: {json}");
}

/// Four files — the exact shape the user uploads (4 PDFs in the browser).
#[tokio::test]
async fn four_files_reach_account_lookup() {
    let (app, _) = test_router();
    let parts = make_parts(&["a.csv", "b.csv", "c.csv", "d.csv"]);
    let (status, json) = post_parse(app, multipart_body(&parts)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {json}");
    assert_eq!(json["code"], "account_not_found", "body: {json}");
}

/// MAX_FILES + 1 — the route caps at 5 files; the 6th must be rejected
/// cleanly with `too_many_files`, not crash the multipart loop.
#[tokio::test]
async fn six_files_rejected_as_too_many() {
    let (app, _) = test_router();
    let parts = make_parts(&["1.csv", "2.csv", "3.csv", "4.csv", "5.csv", "6.csv"]);
    let (status, json) = post_parse(app, multipart_body(&parts)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {json}");
    assert_eq!(json["code"], "too_many_files", "body: {json}");
}

/// Body just over 2 MB (above Axum's default 2 MB limit, below our 50 MB
/// override). If the `DefaultBodyLimit` layer ever drops off the parse
/// route, this test catches it.
#[tokio::test]
async fn large_body_under_route_limit_is_accepted() {
    let (app, _) = test_router();
    let big: Vec<u8> = vec![b'a'; 3 * 1024 * 1024]; // 3 MB body chunk
    let mut parts: Vec<Part> = vec![Part::File {
        name: "files[]",
        filename: "big.csv",
        content_type: "text/csv",
        body: &big,
    }];
    parts.push(Part::Text {
        name: "account_id",
        body: "missing-account",
    });
    parts.push(Part::Text {
        name: "hints",
        body: HINTS,
    });
    let (status, json) = post_parse(app, multipart_body(&parts)).await;
    // Either account_not_found (route accepted body, just no account) or
    // bad_request from preprocessing — both prove we got past the body
    // limit layer. What we MUST NOT see is 413 Payload Too Large.
    assert_ne!(status, StatusCode::PAYLOAD_TOO_LARGE, "body: {json}");
    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {json}");
}
