//! `POST /api/income-sources/:id/sync` (plan 30 section 8) against a local
//! stub of the shop app's endpoint. The sync reads its URL and token from the
//! process environment, so every test takes `ENV_LOCK`. All data synthetic.

use std::sync::{Arc, Mutex};

use axum::body::{Body, to_bytes};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, Method, Request, StatusCode};
use axum::routing::get;
use fynance::server::build_router;
use fynance::storage::Db;
use serde_json::{Value, json};
use std::collections::HashMap;
use tempfile::tempdir;
use tower::ServiceExt;

static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

const TOKEN: &str = "stub-token-123";

/// One request as the stub saw it: the Authorization header and the query.
type Seen = (Option<String>, HashMap<String, String>);

#[derive(Clone)]
struct Stub {
    status: u16,
    body: String,
    seen: Arc<Mutex<Vec<Seen>>>,
}

async fn stub_handler(
    State(stub): State<Stub>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> (StatusCode, String) {
    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    stub.seen.lock().unwrap().push((auth, query));
    (
        StatusCode::from_u16(stub.status).unwrap(),
        stub.body.clone(),
    )
}

/// Serve `body` with `status` on a random local port; returns the URL.
async fn start_stub(status: u16, body: Value) -> (String, Stub) {
    let stub = Stub {
        status,
        body: body.to_string(),
        seen: Arc::new(Mutex::new(Vec::new())),
    };
    let app = axum::Router::new()
        .route("/income", get(stub_handler))
        .with_state(stub.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}/income"), stub)
}

fn configure(url: &str) {
    unsafe {
        std::env::set_var("FYNANCE_SHOP_SYNC_URL", url);
        std::env::set_var("FYNANCE_SHOP_SYNC_TOKEN", TOKEN);
    }
}

fn unconfigure() {
    unsafe {
        std::env::remove_var("FYNANCE_SHOP_SYNC_URL");
        std::env::remove_var("FYNANCE_SHOP_SYNC_TOKEN");
    }
}

fn test_router() -> axum::Router {
    let dir = tempdir().unwrap();
    let path = dir.keep().join("income_sync_test.db");
    let db = Db::open(&path).unwrap();
    db.create_profile("alex", "Alex").unwrap();
    build_router(Arc::new(Mutex::new(db)), true)
}

async fn call(
    app: &axum::Router,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(match body {
            Some(b) => Body::from(b.to_string()),
            None => Body::empty(),
        })
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let text = String::from_utf8_lossy(&bytes).to_string();
    (
        status,
        serde_json::from_str(&text).unwrap_or(Value::String(text)),
    )
}

async fn create_source(app: &axum::Router, source_type: &str, sync_kind: Option<&str>) -> String {
    let mut body = json!({ "profile_id": "alex", "source_type": source_type, "name": "Shop" });
    if let Some(kind) = sync_kind {
        body["sync_kind"] = json!(kind);
    }
    let (status, source) = call(app, Method::POST, "/api/income-sources", Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "{source}");
    source["id"].as_str().unwrap().to_string()
}

async fn sync(app: &axum::Router, id: &str, dry_run: bool) -> (StatusCode, Value) {
    call(
        app,
        Method::POST,
        &format!("/api/income-sources/{id}/sync"),
        Some(json!({ "from": "2025-04-06", "to": "2025-05-31", "dry_run": dry_run })),
    )
    .await
}

async fn records(app: &axum::Router, id: &str) -> Vec<Value> {
    let (status, body) = call(
        app,
        Method::GET,
        &format!("/api/income-records?profile_id=alex&source_id={id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["items"].as_array().unwrap().clone()
}

/// April is split at the 6th; the endpoint widened the range to start on 1 April.
fn shop_body(sales_pence: i64) -> Value {
    json!({
        "basis": "order_date",
        "currency": "GBP",
        "generated_at": "2025-06-01T09:00:00Z",
        "periods": [
            { "start": "2025-04-01", "end": "2025-04-05", "sales_pence": 1500,
              "refunds_pence": 0, "platform_fees_pence": 100,
              "platform_postage_pence": 0, "costing_materials_pence": 0,
              "costing_packaging_pence": 0, "costing_postage_pence": 0,
              "costing_other_pence": 0, "order_count": 1,
              "orders_missing_fee": 0, "orders_missing_costs": 0 },
            { "start": "2025-04-06", "end": "2025-04-30", "sales_pence": sales_pence,
              "refunds_pence": 500, "platform_fees_pence": 964,
              "platform_postage_pence": 250, "costing_materials_pence": 3000,
              "costing_packaging_pence": 0, "costing_postage_pence": 0,
              "costing_other_pence": 0, "order_count": 5,
              "orders_missing_fee": 2, "orders_missing_costs": 1 },
            { "start": "2025-05-01", "end": "2025-05-31", "sales_pence": 0,
              "refunds_pence": 0, "platform_fees_pence": 0,
              "platform_postage_pence": 0, "costing_materials_pence": 0,
              "costing_packaging_pence": 0, "costing_postage_pence": 0,
              "costing_other_pence": 0, "order_count": 0,
              "orders_missing_fee": 0, "orders_missing_costs": 0 }
        ],
        "late_refunds": [
            { "order_month": "2025-03", "refund_date": "2025-04-10", "amount_pence": 500 }
        ]
    })
}

fn amount_of(record: &Value, line_type: &str) -> Option<String> {
    record["line_items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["line_type"] == line_type)
        .map(|i| i["amount"].as_str().unwrap().to_string())
}

#[tokio::test]
async fn sync_sends_auth_maps_periods_and_upserts_idempotently() {
    let _guard = ENV_LOCK.lock().await;
    let (url, stub) = start_stub(200, shop_body(12050)).await;
    configure(&url);
    let app = test_router();
    let id = create_source(&app, "self_employment", Some("shop_app")).await;

    // A manual record in the same year must survive every sync.
    let (status, manual) = call(
        &app,
        Method::POST,
        "/api/income-records",
        Some(
            json!({ "source_id": id, "period_start": "2025-06-01", "period_end": "2025-06-30",
                     "line_items": [{ "line_type": "sales", "amount": "10.00" }] }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{manual}");

    // Dry run writes nothing but reports what would happen.
    let (status, dry) = sync(&app, &id, true).await;
    assert_eq!(status, StatusCode::OK, "{dry}");
    assert_eq!(dry["dry_run"], true);
    assert_eq!(dry["created"], 3);
    assert_eq!(records(&app, &id).await.len(), 1);

    let (status, first) = sync(&app, &id, false).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(
        (
            first["created"].clone(),
            first["updated"].clone(),
            first["unchanged"].clone()
        ),
        (json!(3), json!(0), json!(0))
    );

    // Auth header and query reached the stub on every call.
    let seen = stub.seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 2);
    for (auth, query) in &seen {
        assert_eq!(auth.as_deref(), Some(format!("Bearer {TOKEN}").as_str()));
        assert_eq!(query["from"], "2025-04-06");
        assert_eq!(query["to"], "2025-05-31");
    }

    // April split: periods are mapped as returned (the first starts before `from`).
    let refs: Vec<&str> = first["periods"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["external_ref"].as_str().unwrap())
        .collect();
    assert_eq!(
        refs,
        [
            "2025-04-01..2025-04-05",
            "2025-04-06..2025-04-30",
            "2025-05-01..2025-05-31"
        ]
    );
    let stored = records(&app, &id).await;
    assert_eq!(stored.len(), 4);
    let april = stored
        .iter()
        .find(|r| r["external_ref"] == "2025-04-06..2025-04-30")
        .unwrap();
    assert_eq!(april["origin"], "shop_sync");
    assert_eq!(april["tax_year"], "2025-26");
    assert_eq!(amount_of(april, "sales").as_deref(), Some("120.50"));
    assert_eq!(amount_of(april, "platform_fees").as_deref(), Some("9.64"));
    assert_eq!(
        amount_of(april, "costing_materials").as_deref(),
        Some("30.00")
    );
    assert_eq!(
        amount_of(april, "costing_other"),
        None,
        "zero amounts are omitted"
    );
    let last_year = stored
        .iter()
        .find(|r| r["external_ref"] == "2025-04-01..2025-04-05")
        .unwrap();
    assert_eq!(last_year["tax_year"], "2024-25");
    let empty_month = stored
        .iter()
        .find(|r| r["external_ref"] == "2025-05-01..2025-05-31")
        .unwrap();
    assert!(empty_month["line_items"].as_array().unwrap().is_empty());

    // Late refunds are reported; missing fee/costs become warnings.
    assert_eq!(first["late_refunds"][0]["order_month"], "2025-03");
    assert_eq!(first["late_refunds"][0]["amount"], "5.00");
    let warnings = first["warnings"].as_array().unwrap();
    assert_eq!(warnings.len(), 2, "{warnings:?}");
    assert!(warnings[0].as_str().unwrap().contains("2 orders"));

    // Same data again: nothing changes.
    let (_, second) = sync(&app, &id, false).await;
    assert_eq!(
        (
            second["created"].clone(),
            second["updated"].clone(),
            second["unchanged"].clone()
        ),
        (json!(0), json!(0), json!(3))
    );
    assert_eq!(records(&app, &id).await.len(), 4);

    // Changed figures replace the line items of the same record.
    let (url, _stub) = start_stub(200, shop_body(20000)).await;
    configure(&url);
    let (_, third) = sync(&app, &id, false).await;
    assert_eq!(
        (
            third["created"].clone(),
            third["updated"].clone(),
            third["unchanged"].clone()
        ),
        (json!(0), json!(1), json!(2))
    );
    let stored = records(&app, &id).await;
    assert_eq!(stored.len(), 4);
    let april = stored
        .iter()
        .find(|r| r["external_ref"] == "2025-04-06..2025-04-30")
        .unwrap();
    assert_eq!(amount_of(april, "sales").as_deref(), Some("200.00"));
    let manual_after = stored.iter().find(|r| r["origin"] == "manual").unwrap();
    assert_eq!(amount_of(manual_after, "sales").as_deref(), Some("10.00"));
    unconfigure();
}

#[tokio::test]
async fn sync_upstream_non_200_is_502_with_status_only() {
    let _guard = ENV_LOCK.lock().await;
    let (url, _stub) = start_stub(503, json!({ "detail": "secret-internal-detail" })).await;
    configure(&url);
    let app = test_router();
    let id = create_source(&app, "self_employment", Some("shop_app")).await;
    let (status, body) = sync(&app, &id, false).await;
    unconfigure();
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    assert_eq!(body["code"], "sync_upstream_error");
    let text = body.to_string();
    assert!(text.contains("503"), "{text}");
    assert!(!text.contains("secret-internal-detail"), "{text}");
    assert!(!text.contains(TOKEN), "{text}");
    assert!(records(&app, &id).await.is_empty());
}

#[tokio::test]
async fn sync_without_config_is_400() {
    let _guard = ENV_LOCK.lock().await;
    unconfigure();
    let app = test_router();
    let id = create_source(&app, "self_employment", Some("shop_app")).await;
    let (status, body) = sync(&app, &id, false).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "sync_not_configured");

    // A URL alone is not enough.
    unsafe { std::env::set_var("FYNANCE_SHOP_SYNC_URL", "http://127.0.0.1:1/income") };
    let (status, body) = sync(&app, &id, false).await;
    unconfigure();
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "sync_not_configured");
}

#[tokio::test]
async fn sync_rejects_wrong_source_and_bad_range() {
    let _guard = ENV_LOCK.lock().await;
    let (url, stub) = start_stub(200, shop_body(100)).await;
    configure(&url);
    let app = test_router();

    let employment = create_source(&app, "employment", None).await;
    let (status, body) = sync(&app, &employment, false).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "sync_not_supported");

    let unsynced = create_source(&app, "self_employment", None).await;
    let (status, body) = sync(&app, &unsynced, false).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "sync_not_supported");

    let synced = create_source(&app, "self_employment", Some("shop_app")).await;
    let (status, body) = call(
        &app,
        Method::POST,
        &format!("/api/income-sources/{synced}/sync"),
        Some(json!({ "from": "2025-05-31", "to": "2025-04-06" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "invalid_range");

    let (status, body) = sync(&app, "no-such-source", false).await;
    unconfigure();
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "source_not_found");
    assert!(
        stub.seen.lock().unwrap().is_empty(),
        "no upstream call was made"
    );
}

/// Serve a 302 to `location` on a random local port; returns the URL.
async fn start_redirect(location: String) -> (String, Arc<Mutex<usize>>) {
    let hits = Arc::new(Mutex::new(0usize));
    let counter = hits.clone();
    let app = axum::Router::new().route(
        "/income",
        get(move || {
            let location = location.clone();
            let counter = counter.clone();
            async move {
                *counter.lock().unwrap() += 1;
                (StatusCode::FOUND, [("location", location)], String::new())
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}/income"), hits)
}

#[tokio::test]
async fn sync_does_not_follow_redirects() {
    let _guard = ENV_LOCK.lock().await;
    let (target, target_stub) = start_stub(200, shop_body(100)).await;
    let (url, hits) = start_redirect(target).await;
    configure(&url);
    let app = test_router();
    let id = create_source(&app, "self_employment", Some("shop_app")).await;
    let (status, body) = sync(&app, &id, false).await;
    unconfigure();
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    assert!(body.to_string().contains("302"), "{body}");
    assert_eq!(*hits.lock().unwrap(), 1);
    assert!(target_stub.seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn sync_currency_change_is_409_and_changes_nothing() {
    let _guard = ENV_LOCK.lock().await;
    let (url, _stub) = start_stub(200, shop_body(100)).await;
    configure(&url);
    let app = test_router();
    let id = create_source(&app, "self_employment", Some("shop_app")).await;
    let (status, body) = sync(&app, &id, false).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let before = records(&app, &id).await;

    let (status, body) = call(
        &app,
        Method::POST,
        "/api/currencies",
        Some(json!({ "code": "EUR", "fx_rate": "0.85" })),
    )
    .await;
    assert!(status.is_success(), "{body}");
    let mut eur = shop_body(999);
    eur["currency"] = json!("EUR");
    let (url, _stub) = start_stub(200, eur).await;
    configure(&url);
    let (status, body) = sync(&app, &id, false).await;
    unconfigure();
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "sync_currency_changed");
    assert_eq!(records(&app, &id).await, before);
}

#[tokio::test]
async fn sync_commit_rolls_back_when_a_later_period_is_bad() {
    let _guard = ENV_LOCK.lock().await;
    let mut bad = shop_body(100);
    let first = bad["periods"][0].clone();
    bad["periods"] = json!([bad["periods"][1].clone(), first.clone(), first]);
    let (url, _stub) = start_stub(200, bad).await;
    configure(&url);
    let app = test_router();
    let id = create_source(&app, "self_employment", Some("shop_app")).await;
    let (status, body) = sync(&app, &id, false).await;
    unconfigure();
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    assert!(body.to_string().contains("appears twice"), "{body}");
    assert!(records(&app, &id).await.is_empty());
}

#[tokio::test]
async fn sync_warns_on_other_basis_and_rejects_negative_pence() {
    let _guard = ENV_LOCK.lock().await;
    let mut other = shop_body(100);
    other["basis"] = json!("ship_date");
    let (url, _stub) = start_stub(200, other).await;
    configure(&url);
    let app = test_router();
    let id = create_source(&app, "self_employment", Some("shop_app")).await;
    let (status, body) = sync(&app, &id, true).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["warnings"].to_string().contains("ship_date"), "{body}");

    let (url, _stub) = start_stub(200, shop_body(-100)).await;
    configure(&url);
    let (status, body) = sync(&app, &id, false).await;
    unconfigure();
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    assert!(records(&app, &id).await.is_empty());
}

#[tokio::test]
async fn sync_refuses_bad_urls_and_long_ranges() {
    let _guard = ENV_LOCK.lock().await;
    let app = test_router();
    let id = create_source(&app, "self_employment", Some("shop_app")).await;
    for url in [
        "shop.example.com/income",
        "ftp://shop.example.com/income",
        "http://shop.example.com/income",
    ] {
        configure(url);
        let (status, body) = sync(&app, &id, false).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{url}: {body}");
        assert_eq!(body["code"], "sync_not_configured", "{url}");
        assert!(!body.to_string().contains("shop.example.com"), "{body}");
    }
    unconfigure();

    let (status, body) = call(
        &app,
        Method::POST,
        &format!("/api/income-sources/{id}/sync"),
        Some(json!({ "from": "2020-04-06", "to": "2025-04-05" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "invalid_range");
}
