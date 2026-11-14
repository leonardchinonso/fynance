//! Tax-document parse validation (no LLM reachable) and the commit route
//! `POST /api/income-records/import`. All data synthetic.

use std::sync::{Arc, Mutex};

use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use fynance::server::build_router;
use fynance::storage::Db;
use serde_json::{Value, json};
use tempfile::tempdir;
use tower::ServiceExt;

const BOUNDARY: &str = "----TaxBoundary";
const PDF: &[u8] = include_bytes!("fixtures/tax/synthetic_payslips.pdf");
const TAX_HINTS: &str = r#"{"return_type":{"transactions":false,"holdings":{"enabled":false},"investments":false,"tax_documents":true}}"#;
const MIXED_HINTS: &str = r#"{"return_type":{"transactions":true,"holdings":{"enabled":false},"investments":false,"tax_documents":true}}"#;

fn router() -> axum::Router {
    let dir = tempdir().unwrap();
    let db = Db::open(&dir.keep().join("tax_parse.db")).unwrap();
    db.create_profile("alex", "Alex").unwrap();
    build_router(Arc::new(Mutex::new(db)), true)
}

fn multipart(files: &[(&str, &[u8])], fields: &[(&str, &str)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (name, body) in files {
        out.extend_from_slice(format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"files[]\"; filename=\"{name}\"\r\nContent-Type: application/octet-stream\r\n\r\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\r\n");
    }
    for (name, body) in fields {
        out.extend_from_slice(
            format!(
                "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{body}\r\n"
            )
            .as_bytes(),
        );
    }
    out.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    out
}

async fn send(app: &axum::Router, req: Request<Body>) -> (StatusCode, Value) {
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn parse(
    app: &axum::Router,
    files: &[(&str, &[u8])],
    fields: &[(&str, &str)],
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::POST)
        .uri("/api/parse")
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .body(Body::from(multipart(files, fields)))
        .unwrap();
    send(app, req).await
}

async fn import(app: &axum::Router, body: Value) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::POST)
        .uri("/api/income-records/import")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    send(app, req).await
}

fn code(v: &Value) -> &str {
    v["code"].as_str().unwrap_or_default()
}

#[tokio::test]
async fn tax_parse_request_validation() {
    let app = router();
    let (s, b) = parse(
        &app,
        &[("a.pdf", PDF)],
        &[("profile_id", "alex"), ("hints", MIXED_HINTS)],
    )
    .await;
    assert_eq!(
        (s, code(&b)),
        (StatusCode::BAD_REQUEST, "invalid_return_type")
    );

    let (s, b) = parse(&app, &[("a.pdf", PDF)], &[("hints", TAX_HINTS)]).await;
    assert_eq!(
        (s, code(&b)),
        (StatusCode::BAD_REQUEST, "missing_profile_id")
    );

    let (s, b) = parse(
        &app,
        &[("a.pdf", PDF)],
        &[("profile_id", "nobody"), ("hints", TAX_HINTS)],
    )
    .await;
    assert_eq!(
        (s, code(&b)),
        (StatusCode::BAD_REQUEST, "profile_not_found")
    );

    let (s, b) = parse(
        &app,
        &[("a.csv", b"a,b\n1,2\n")],
        &[("profile_id", "alex"), ("hints", TAX_HINTS)],
    )
    .await;
    assert_eq!(
        (s, code(&b)),
        (StatusCode::BAD_REQUEST, "unsupported_tax_document_format")
    );

    let names: Vec<String> = (0..16).map(|i| format!("p{i}.pdf")).collect();
    let sixteen: Vec<(&str, &[u8])> = names.iter().map(|n| (n.as_str(), PDF)).collect();
    let (s, b) = parse(
        &app,
        &sixteen,
        &[("profile_id", "alex"), ("hints", TAX_HINTS)],
    )
    .await;
    assert_eq!((s, code(&b)), (StatusCode::BAD_REQUEST, "too_many_files"));
}

#[tokio::test]
async fn fifteen_pdfs_pass_the_cap_without_an_account() {
    if std::env::var("FYNANCE_ANTHROPIC_API_KEY").is_ok()
        || std::env::var("FYNANCE_CLAUDE_CODE_OAUTH_TOKEN").is_ok()
    {
        return;
    }
    let app = router();
    let names: Vec<String> = (0..15).map(|i| format!("p{i}.pdf")).collect();
    let files: Vec<(&str, &[u8])> = names.iter().map(|n| (n.as_str(), PDF)).collect();
    let (s, b) = parse(
        &app,
        &files,
        &[("profile_id", "alex"), ("hints", TAX_HINTS)],
    )
    .await;
    assert_eq!((s, code(&b)), (StatusCode::BAD_REQUEST, "provider_config"));
}

fn payslip(pay_date: &str, period_end: &str) -> Value {
    json!({
        "document_type": "payslip", "source_name": "Example Widgets Ltd",
        "paye_reference": "123/AB456", "tax_year": "2025-26",
        "pay_date": pay_date, "period_start": "2025-06-01", "period_end": period_end,
        "currency": "GBP",
        "line_items": [
            {"line_type": "taxable_pay", "amount": "3000.00", "basis": "period"},
            {"line_type": "paye_tax", "amount": "400.00", "basis": "period"},
            {"line_type": "taxable_pay", "amount": "9000.00", "basis": "year_to_date"}
        ]
    })
}

fn p60() -> Value {
    json!({
        "document_type": "annual_certificate", "source_name": "example widgets",
        "tax_year": "2025-26", "currency": "GBP",
        "line_items": [
            {"line_type": "taxable_pay", "amount": "36000.00", "basis": "period"},
            {"line_type": "paye_tax", "amount": "4800.00", "basis": "period"}
        ]
    })
}

#[tokio::test]
async fn commit_writes_records_and_checks_and_skips_duplicates() {
    let app = router();
    let docs = json!([
        payslip("2025-06-27", "2025-06-30"),
        payslip("2025-06-27", "2025-06-30"),
        p60(),
        p60()
    ]);
    let (s, b) = import(&app, json!({"profile_id": "alex", "documents": docs})).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(b["inserted"], 2);
    assert_eq!(b["duplicates"], 1);
    assert_eq!(b["errors"].as_array().unwrap().len(), 1);
    assert_eq!(b["errors"][0]["index"], 3);

    let req = Request::builder()
        .uri("/api/income-sources?profile_id=alex")
        .body(Body::empty())
        .unwrap();
    let (_, sources) = send(&app, req).await;
    assert_eq!(sources["items"].as_array().unwrap().len(), 1);
    assert_eq!(
        sources["items"][0]["identifiers"]["paye_reference"],
        "123/AB456"
    );

    let req = Request::builder()
        .uri("/api/income-records?profile_id=alex")
        .body(Body::empty())
        .unwrap();
    let (_, records) = send(&app, req).await;
    let items = records["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["origin"], "parse");
    assert_eq!(items[0]["tax_year"], "2025-26");

    let req = Request::builder()
        .uri("/api/income-checks?profile_id=alex")
        .body(Body::empty())
        .unwrap();
    let (_, checks) = send(&app, req).await;
    assert_eq!(checks["items"][0]["check_type"], "p60");

    // a second commit of the same payslip is a duplicate, not an error
    let (_, b) = import(
        &app,
        json!({"profile_id": "alex", "documents": [payslip("2025-06-27", "2025-06-30")]}),
    )
    .await;
    assert_eq!(
        (b["inserted"].as_u64(), b["duplicates"].as_u64()),
        (Some(0), Some(1))
    );
}

#[tokio::test]
async fn commit_reports_bad_rows_and_unknown_profile() {
    let app = router();
    let mut bad_tx = payslip("2025-07-25", "2025-07-31");
    bad_tx["transaction_id"] = json!("missing");
    let mut negative = payslip("2025-08-29", "2025-08-31");
    negative["line_items"][0]["amount"] = json!("-1");
    let (s, b) = import(&app, json!({"profile_id": "alex", "documents": [bad_tx, negative, payslip("2025-09-26", "2025-09-30")]})).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(b["inserted"], 1);
    assert_eq!(b["errors"].as_array().unwrap().len(), 2);

    let (s, b) = import(&app, json!({"profile_id": "nobody", "documents": []})).await;
    assert_eq!(
        (s, code(&b)),
        (StatusCode::BAD_REQUEST, "profile_not_found")
    );
}
