//! Integration tests for `POST /api/parse` on the OpenAI-compatible provider
//! (plan 30 Part 2).
//!
//! Each case runs a one-shot fake chat server on loopback (never
//! api.openai.com), a temp DB with a profile and an account, and drives the
//! real router. The provider is constructed by the route from env, so cases
//! set env under a file mutex and run one at a time. Part 2 pins
//! `FYNANCE_OPENAI_STREAM=false`; Part 3 adds the streamed default case.

// The env mutex is deliberately held across awaits: the route reads the
// provider config mid-request, so a case's env must stay pinned while its
// request runs.
#![allow(clippy::await_holding_lock)]

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use fynance::model::{Account, AccountType};
use fynance::server::build_router;
use fynance::storage::Db;
use rust_decimal::Decimal;
use tempfile::tempdir;
use tower::ServiceExt;

/// Env-mutating cases share the process env; run them one at a time.
static ENV_MUTEX: Mutex<()> = Mutex::new(());

const BOUNDARY: &str = "----OpenAIParityBoundary7";

const TRANSACTIONS_AND_HOLDINGS_HINTS: &str = r#"{"return_type":{"transactions":true,"holdings":{"enabled":true,"period":null},"investments":false}}"#;

const UNIFIED_TOOL_JSON: &str = r#"{"transactions":[{"date":"2026-11-02","description":"Corner Shop","amount":"-12.00","currency":"GBP","row_confidence":0.9},{"date":"2026-11-05","description":"Salary ACME Ltd","amount":"2450.00","currency":"GBP","row_confidence":0.9}],"holdings":[{"symbol":"VWRP","name":"Vanguard Global ETF","holding_type":"etf","quantity":"10","price_per_unit":"50.00","value":"500.00","currency":"GBP","row_confidence":0.9}],"investments":[]}"#;

fn tool_call_body(arguments: &str) -> String {
    serde_json::to_string(&serde_json::json!({
        "choices": [{
            "message": {
                "tool_calls": [{
                    "function": { "name": "extract_unified", "arguments": arguments }
                }]
            },
            "finish_reason": "tool_calls"
        }],
        "usage": {"prompt_tokens": 100, "completion_tokens": 200}
    }))
    .unwrap()
}

const TINY_CSV: &[u8] = b"date,description,amount\n2026-11-02,Corner Shop,-12.00\n";

/// One-shot loopback chat server: answers the first request with `body`
/// (as JSON) and hands the raw request back through the join handle.
fn serve_once(body: &'static str) -> (String, std::thread::JoinHandle<String>) {
    use std::io::{Read, Write};
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut raw = Vec::new();
        let mut buf = [0u8; 8192];
        while !request_complete(&raw) {
            match stream.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => raw.extend_from_slice(&buf[..n]),
            }
        }
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes());
        String::from_utf8_lossy(&raw).into_owned()
    });
    (format!("http://{addr}"), handle)
}

fn request_complete(raw: &[u8]) -> bool {
    let Some(header_end) = raw.windows(4).position(|w| w == b"\r\n\r\n") else {
        return false;
    };
    let content_length = String::from_utf8_lossy(&raw[..header_end])
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())?
        })
        .unwrap_or(0);
    raw.len() >= header_end + 4 + content_length
}

fn request_json(raw: &str) -> serde_json::Value {
    let body = raw
        .split_once("\r\n\r\n")
        .expect("request had no header terminator")
        .1;
    serde_json::from_str(body).expect("chat request body must be JSON")
}

fn test_db() -> Arc<Mutex<Db>> {
    let dir = tempdir().unwrap();
    let path = dir.keep().join("openai_parity.db");
    let db = Db::open(&path).unwrap();
    db.create_profile("zaida", "Zaida").unwrap();
    db.upsert_account(&Account {
        id: "test-current".to_string(),
        name: "Test Current".to_string(),
        institution: "Test Bank".to_string(),
        account_type: AccountType::Checking,
        currency: "GBP".to_string(),
        balance: Some(Decimal::new(1000, 2)),
        balance_date: None,
        is_active: true,
        notes: None,
        profile_ids: vec!["zaida".to_string()],
        is_stale: None,
        is_available: false,
    })
    .unwrap();
    Arc::new(Mutex::new(db))
}

fn multipart_request(file: (&str, &str, &[u8]), hints: &str) -> Vec<u8> {
    let (filename, content_type, bytes) = file;
    let mut out = Vec::new();
    out.extend_from_slice(
        format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"files[]\"; filename=\"{filename}\"\r\nContent-Type: {content_type}\r\n\r\n").as_bytes(),
    );
    out.extend_from_slice(bytes);
    out.extend_from_slice(
        format!(
            "\r\n--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"account_id\"\r\n\r\ntest-current\r\n\
             --{BOUNDARY}\r\nContent-Disposition: form-data; name=\"hints\"\r\n\r\n{hints}\r\n\
             --{BOUNDARY}--\r\n"
        )
        .as_bytes(),
    );
    out
}

async fn post_parse(db: Arc<Mutex<Db>>, body: Vec<u8>) -> (StatusCode, serde_json::Value) {
    let app = build_router(db, true);
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
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

fn env_guard() -> std::sync::MutexGuard<'static, ()> {
    ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner())
}

struct EnvCase;

impl EnvCase {
    /// Set the shared OpenAI env for a case; every provider built by the
    /// route during the case points at `base_url`, a loopback fake.
    fn start(base_url: &str) {
        for key in [
            "FYNANCE_PARSE_PROVIDER",
            "FYNANCE_OPENAI_API_KEY",
            "FYNANCE_OPENAI_BASE_URL",
            "FYNANCE_OPENAI_TEXT_MODEL",
            "FYNANCE_OPENAI_PDF_MODEL",
            "FYNANCE_OPENAI_STREAM",
            "FYNANCE_OPENAI_PDF_MODE",
            "FYNANCE_OPENAI_PDF_MAX_PAGES",
            "FYNANCE_PDF_EXTRACT_CMD",
            "FYNANCE_PDF_EXTRACT_ARGS",
            "FYNANCE_PDF_EXTRACT_TIMEOUT_SECS",
        ] {
            unsafe { std::env::remove_var(key) };
        }
        unsafe {
            std::env::set_var("FYNANCE_PARSE_PROVIDER", "openai");
            std::env::set_var("FYNANCE_OPENAI_BASE_URL", format!("{base_url}/v1"));
            std::env::set_var("FYNANCE_OPENAI_TEXT_MODEL", "test-model");
            std::env::set_var("FYNANCE_OPENAI_PDF_MODEL", "test-model");
            std::env::set_var("FYNANCE_OPENAI_STREAM", "false");
        }
    }

    fn set(key: &str, value: &str) {
        unsafe { std::env::set_var(key, value) };
    }
}

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

/// Write a fake pdf-extract tool that answers like the `Ok` behaviour (copy
/// the fixture manifest and PNGs into `--out`), or the `Garbage` one
/// (write `not json` as the manifest). Fixture paths are baked into the
/// script, so the fake needs no env.
fn write_fake_pdf_tool(dir: &Path, ok: bool) -> PathBuf {
    let fixtures = fixture_path("pdf_extract");
    let fixtures = fixtures.display().to_string();
    let script = if cfg!(windows) {
        if ok {
            format!(
                "@echo off\r\nsetlocal\r\nset \"OUT=%~3\"\r\nif not exist \"%OUT%\" mkdir \"%OUT%\"\r\n\
                 copy /Y \"{fixtures}\\extract.json\" \"%OUT%\" >nul\r\n\
                 copy /Y \"{fixtures}\\page-00*.png\" \"%OUT%\" >nul\r\nexit /b 0\r\n"
            )
        } else {
            "@echo off\r\nsetlocal\r\nset \"OUT=%~3\"\r\nif not exist \"%OUT%\" mkdir \"%OUT%\"\r\n\
             > \"%OUT%\\extract.json\" echo not json\r\nexit /b 0\r\n"
                .to_string()
        }
    } else if ok {
        format!(
            "#!/bin/sh\nOUT=\"$3\"\nmkdir -p \"$OUT\"\ncp '{fixtures}/extract.json' \"$OUT/\"\n\
             cp '{fixtures}'/page-00*.png \"$OUT/\"\nexit 0\n"
        )
    } else {
        "#!/bin/sh\nOUT=\"$3\"\nmkdir -p \"$OUT\"\nprintf 'not json' > \"$OUT/extract.json\"\nexit 0\n"
            .to_string()
    };
    let name = if cfg!(windows) {
        "fake_tool.cmd"
    } else {
        "fake_tool.sh"
    };
    let path = dir.join(name);
    std::fs::write(&path, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    path
}

fn synthetic_pdf() -> Vec<u8> {
    std::fs::read(fixture_path("synthetic_statement.pdf")).expect("fixture present")
}

// ── Cases ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn csv_unified_parse_end_to_end() {
    let _guard = env_guard();
    let (url, server) = serve_once(Box::leak(
        tool_call_body(UNIFIED_TOOL_JSON).into_boxed_str(),
    ));
    EnvCase::start(&url);

    let (status, json) = post_parse(
        test_db(),
        multipart_request(
            ("statement.csv", "text/csv", TINY_CSV),
            TRANSACTIONS_AND_HOLDINGS_HINTS,
        ),
    )
    .await;
    let request = request_json(&server.join().unwrap());

    assert_eq!(status, StatusCode::OK, "body: {json}");
    assert_eq!(
        json["transactions"]["count"],
        serde_json::json!(2),
        "body: {json}"
    );
    assert_eq!(
        json["holdings"]["count"],
        serde_json::json!(1),
        "body: {json}"
    );
    assert_eq!(
        request["model"],
        serde_json::json!("test-model"),
        "chat request: {request}"
    );
    assert_eq!(request["tool_choice"], serde_json::json!("required"));

    let calls = json["metadata"]["estimated_price"]["calls"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert_eq!(
        calls.first().and_then(|c| c["model"].as_str()),
        Some("test-model")
    );
}

#[tokio::test]
async fn truncated_finish_reason_maps_to_response_truncated() {
    let _guard = env_guard();
    let body = serde_json::json!({
        "choices": [{
            "message": {
                "tool_calls": [{
                    "function": { "name": "extract_unified", "arguments": "{\"transactions\":[{\"date\":\"2026-11-" }
                }]
            },
            "finish_reason": "length"
        }],
        "usage": {"prompt_tokens": 50, "completion_tokens": 16384}
    })
    .to_string();
    let (url, server) = serve_once(Box::leak(body.into_boxed_str()));
    EnvCase::start(&url);

    let (status, json) = post_parse(
        test_db(),
        multipart_request(
            ("statement.csv", "text/csv", TINY_CSV),
            TRANSACTIONS_AND_HOLDINGS_HINTS,
        ),
    )
    .await;
    server.join().unwrap();

    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {json}");
    assert_eq!(
        json["code"],
        serde_json::json!("response_truncated"),
        "body: {json}"
    );
}

#[tokio::test]
async fn pdf_through_the_extract_tool_reaches_the_model_as_pages() {
    let _guard = env_guard();
    let tool_dir = tempdir().unwrap();
    let tool = write_fake_pdf_tool(tool_dir.path(), true);
    let (url, server) = serve_once(Box::leak(
        tool_call_body(UNIFIED_TOOL_JSON).into_boxed_str(),
    ));
    EnvCase::start(&url);
    EnvCase::set("FYNANCE_PDF_EXTRACT_CMD", &tool.display().to_string());

    let pdf = synthetic_pdf();
    let (status, json) = post_parse(
        test_db(),
        multipart_request(
            ("statement.pdf", "application/pdf", &pdf),
            TRANSACTIONS_AND_HOLDINGS_HINTS,
        ),
    )
    .await;
    let request = request_json(&server.join().unwrap());

    assert_eq!(status, StatusCode::OK, "body: {json}");
    let parts = request["messages"][1]["content"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let image_parts: Vec<&serde_json::Value> = parts
        .iter()
        .filter(|p| p["type"] == serde_json::json!("image_url"))
        .collect();
    assert_eq!(image_parts.len(), 3, "one page image per page: {parts:?}");
    assert!(
        parts.iter().any(|p| {
            p["text"]
                .as_str()
                .unwrap_or_default()
                .contains("Corner Shop")
        }),
        "page 1 text layer must reach the model"
    );
}

#[tokio::test]
async fn pdf_without_extract_tool_is_provider_not_supported() {
    let _guard = env_guard();
    let (url, _server) = serve_once(Box::leak(
        tool_call_body(UNIFIED_TOOL_JSON).into_boxed_str(),
    ));
    EnvCase::start(&url);
    // EnvCase::start removes FYNANCE_PDF_EXTRACT_CMD; the default mode is both.

    let pdf = synthetic_pdf();
    let (status, json) = post_parse(
        test_db(),
        multipart_request(
            ("statement.pdf", "application/pdf", &pdf),
            TRANSACTIONS_AND_HOLDINGS_HINTS,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {json}");
    assert_eq!(
        json["code"],
        serde_json::json!("provider_not_supported"),
        "body: {json}"
    );
    assert!(
        json["error"]
            .as_str()
            .unwrap_or_default()
            .contains("FYNANCE_PDF_EXTRACT_CMD"),
        "body: {json}"
    );
}

#[tokio::test]
async fn pdf_conversion_failure_maps_to_document_conversion_failed() {
    let _guard = env_guard();
    let tool_dir = tempdir().unwrap();
    let tool = write_fake_pdf_tool(tool_dir.path(), false);
    let (url, _server) = serve_once(Box::leak(
        tool_call_body(UNIFIED_TOOL_JSON).into_boxed_str(),
    ));
    EnvCase::start(&url);
    EnvCase::set("FYNANCE_PDF_EXTRACT_CMD", &tool.display().to_string());

    let pdf = synthetic_pdf();
    let (status, json) = post_parse(
        test_db(),
        multipart_request(
            ("statement.pdf", "application/pdf", &pdf),
            TRANSACTIONS_AND_HOLDINGS_HINTS,
        ),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "body: {json}");
    assert_eq!(
        json["code"],
        serde_json::json!("document_conversion_failed"),
        "body: {json}"
    );
}
