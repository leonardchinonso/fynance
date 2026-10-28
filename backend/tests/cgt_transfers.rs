//! Capital gains on shares moved between accounts and between people.
//!
//! A Section 104 pool belongs to a person: shares moved between one person's
//! accounts keep their cost, and shares given to someone else leave the
//! sender's pool. Between spouses the receiver takes over the sender's cost
//! (TCGA s58); between anyone else it is a gift at market value.

use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode};
use fynance::model::{Account, AccountType, CreateInvestmentEventBody};
use fynance::server::build_router;
use fynance::storage::Db;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tempfile::tempdir;
use tower::ServiceExt;

/// Ope holds two GBP accounts, Tomi one.
fn household() -> (axum::Router, Arc<Mutex<Db>>) {
    let path = tempdir().unwrap().keep().join("cgt_transfers.db");
    let db = Db::open(&path).unwrap();
    db.create_profile("ope", "Ope").unwrap();
    db.create_profile("tomi", "Tomi").unwrap();
    for (id, owner) in [("ope-a", "ope"), ("ope-b", "ope"), ("tomi-a", "tomi")] {
        db.create_account(&Account {
            id: id.to_string(),
            name: id.to_string(),
            institution: "Broker".to_string(),
            account_type: AccountType::Investment,
            currency: "GBP".to_string(),
            balance: None,
            balance_date: None,
            is_active: true,
            notes: None,
            profile_ids: vec![owner.to_string()],
            is_stale: None,
            is_available: true,
        })
        .unwrap();
    }
    let shared = Arc::new(Mutex::new(db));
    (build_router(shared.clone(), true), shared)
}

fn event(db: &Arc<Mutex<Db>>, account: &str, kind: &str, date: &str, qty: &str, price: &str) {
    db.lock()
        .unwrap()
        .create_investment_event(&CreateInvestmentEventBody {
            account_id: account.to_string(),
            event_type: kind.to_string(),
            symbol: "PLTR".to_string(),
            date: format!("{date}T00:00:00"),
            quantity: qty.to_string(),
            price_per_share: price.to_string(),
            fee: None,
            currency: "GBP".to_string(),
            fee_currency: None,
            notes: None,
            source_document_ids: Vec::new(),
        })
        .unwrap();
}

async fn call(
    app: &axum::Router,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let builder = Request::builder().method(method).uri(uri);
    let request = match body {
        Some(b) => builder
            .header("content-type", "application/json")
            .body(Body::from(b.to_string())),
        None => builder.body(Body::empty()),
    }
    .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn report(app: &axum::Router, query: &str) -> Value {
    let (status, body) = call(
        app,
        Method::GET,
        &format!("/api/investments/capital-gains?{query}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

async fn marry(app: &axum::Router) {
    let (status, body) = call(
        app,
        Method::PATCH,
        "/api/profiles/ope",
        Some(json!({ "spouse_id": "tomi" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

fn money(v: &Value) -> f64 {
    v.as_str().unwrap().parse().unwrap()
}

/// Ope buys 100 at £10 and 100 at £20 (pool: 200 at £15), then moves 50 to
/// Tomi when the market price is £25, and Tomi sells 40 at £30.
fn gift_fifty_to_tomi(db: &Arc<Mutex<Db>>) {
    event(db, "ope-a", "buy", "2025-01-10", "100", "10");
    event(db, "ope-a", "buy", "2025-01-20", "100", "20");
    event(db, "ope-a", "transfer", "2025-02-01", "-50", "25");
    event(db, "tomi-a", "transfer", "2025-02-01", "50", "25");
    event(db, "tomi-a", "sell", "2025-03-01", "40", "30");
}

#[tokio::test]
async fn shares_moved_between_ones_own_accounts_keep_their_cost() {
    let (app, db) = household();
    event(&db, "ope-a", "buy", "2025-01-10", "100", "10");
    event(&db, "ope-a", "transfer", "2025-02-01", "-100", "20");
    event(&db, "ope-b", "transfer", "2025-02-02", "100", "20");
    event(&db, "ope-b", "sell", "2025-03-01", "100", "30");

    let body = report(&app, "account_id=ope-b").await;

    assert_eq!(money(&body["summary"]["total_proceeds"]), 3000.0);
    assert_eq!(money(&body["summary"]["total_allowable_costs"]), 1000.0);
}

#[tokio::test]
async fn a_transfer_to_a_spouse_carries_the_senders_cost_and_is_not_reported() {
    let (app, db) = household();
    marry(&app).await;
    gift_fifty_to_tomi(&db);

    let tomi = report(&app, "profile_ids=tomi").await;
    assert_eq!(money(&tomi["summary"]["total_proceeds"]), 1200.0);
    assert_eq!(
        money(&tomi["summary"]["total_allowable_costs"]),
        600.0,
        "40 at Ope's £15"
    );
    let pools = tomi["pools"].as_array().unwrap();
    assert_eq!(pools.len(), 1);
    assert_eq!(pools[0]["profile_id"], "tomi");
    assert_eq!(money(&pools[0]["current_shares"]), 10.0);
    assert_eq!(money(&pools[0]["total_allowable_expenditure"]), 150.0);

    let ope = report(&app, "profile_ids=ope").await;
    assert!(
        ope["realized_events"].as_array().unwrap().is_empty(),
        "no gain, no loss"
    );
    assert_eq!(money(&ope["pools"][0]["current_shares"]), 150.0);
    assert_eq!(
        money(&ope["pools"][0]["total_allowable_expenditure"]),
        2250.0
    );
}

#[tokio::test]
async fn a_transfer_to_someone_else_is_a_gift_at_market_value() {
    let (app, db) = household();
    gift_fifty_to_tomi(&db);

    let ope = report(&app, "profile_ids=ope").await;
    let gift = &ope["realized_events"][0];
    assert_eq!(gift["gift_to"], "tomi");
    assert_eq!(
        money(&gift["proceeds"]),
        1250.0,
        "50 at the £25 market price"
    );
    assert_eq!(money(&gift["cost_basis"]), 750.0);

    let tomi = report(&app, "profile_ids=tomi").await;
    assert_eq!(
        money(&tomi["summary"]["total_allowable_costs"]),
        1000.0,
        "40 at £25"
    );
    assert!(tomi["realized_events"][0].get("gift_to").is_none());
}

#[tokio::test]
async fn one_persons_sale_is_never_matched_with_another_persons_purchase() {
    let (app, db) = household();
    marry(&app).await;
    gift_fifty_to_tomi(&db);
    // Ope buys within 30 days after Tomi's sale. With one shared pool this was
    // matched against her sale under the bed-and-breakfast rule.
    event(&db, "ope-a", "buy", "2025-03-05", "40", "50");

    let all = report(&app, "").await;

    let tomi_sale: Vec<&Value> = all["realized_events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["quantity"] == "40")
        .collect();
    assert_eq!(tomi_sale.len(), 1);
    assert_eq!(tomi_sale[0]["rule_applied"], "S104 Pool");
    let mut owners: Vec<&str> = all["pools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["profile_id"].as_str().unwrap())
        .collect();
    owners.sort();
    assert_eq!(owners, ["ope", "tomi"]);
}

#[tokio::test]
async fn giving_more_than_the_sender_holds_is_refused() {
    let (app, db) = household();
    marry(&app).await;
    event(&db, "ope-a", "buy", "2025-01-10", "10", "10");
    event(&db, "ope-a", "transfer", "2025-02-01", "-50", "25");
    event(&db, "tomi-a", "transfer", "2025-02-01", "50", "25");

    let (status, body) = call(
        &app,
        Method::GET,
        "/api/investments/capital-gains?profile_ids=tomi",
        None,
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "transfer_exceeds_holding", "{body}");
}

#[tokio::test]
async fn the_spouse_link_is_set_and_cleared_on_both_profiles() {
    let (app, _db) = household();
    let spouses = |profiles: &Value| -> Vec<(String, Value)> {
        profiles
            .as_array()
            .unwrap()
            .iter()
            .filter(|p| p["id"] == "ope" || p["id"] == "tomi")
            .map(|p| {
                (
                    p["id"].as_str().unwrap().to_string(),
                    p["spouse_id"].clone(),
                )
            })
            .collect()
    };

    marry(&app).await;
    let (_, profiles) = call(&app, Method::GET, "/api/profiles", None).await;
    assert_eq!(
        spouses(&profiles),
        [
            ("ope".to_string(), json!("tomi")),
            ("tomi".to_string(), json!("ope"))
        ]
    );

    let (status, _) = call(
        &app,
        Method::PATCH,
        "/api/profiles/tomi",
        Some(json!({ "spouse_id": null })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, profiles) = call(&app, Method::GET, "/api/profiles", None).await;
    assert_eq!(
        spouses(&profiles),
        [
            ("ope".to_string(), Value::Null),
            ("tomi".to_string(), Value::Null)
        ]
    );

    let (status, _) = call(
        &app,
        Method::PATCH,
        "/api/profiles/ope",
        Some(json!({ "spouse_id": "ope" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = call(
        &app,
        Method::PATCH,
        "/api/profiles/ope",
        Some(json!({ "spouse_id": "nobody" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
