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

/// A GBP PLTR event at midnight on `date`, for a test to adjust before [`insert`].
fn row(account: &str, kind: &str, date: &str, qty: &str, price: &str) -> CreateInvestmentEventBody {
    CreateInvestmentEventBody {
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
    }
}

fn insert(db: &Arc<Mutex<Db>>, body: CreateInvestmentEventBody) {
    db.lock().unwrap().create_investment_event(&body).unwrap();
}

fn event(db: &Arc<Mutex<Db>>, account: &str, kind: &str, date: &str, qty: &str, price: &str) {
    insert(db, row(account, kind, date, qty, price));
}

/// A USD event, as most of the household's real holdings are.
fn usd_event(
    db: &Arc<Mutex<Db>>,
    account: &str,
    symbol: &str,
    (kind, date): (&str, &str),
    qty: &str,
    price: &str,
) {
    insert(
        db,
        CreateInvestmentEventBody {
            symbol: symbol.to_string(),
            currency: "USD".to_string(),
            ..row(account, kind, date, qty, price)
        },
    );
}

/// Configure USD and store a USD->GBP rate for each of `rates`' dates.
async fn usd_rates(app: &axum::Router, rates: &[(&str, &str)]) {
    let (status, body) = call(
        app,
        Method::POST,
        "/api/currencies",
        Some(json!({ "code": "USD", "fx_rate": "0.8" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    if rates.is_empty() {
        return;
    }
    let rates: Vec<Value> = rates
        .iter()
        .map(|(date, rate)| json!({ "base": "USD", "date": date, "rate": rate }))
        .collect();
    let (status, body) = call(
        app,
        Method::POST,
        "/api/exchange-rates",
        Some(json!({ "rates": rates })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
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

async fn try_report(app: &axum::Router, query: &str) -> (StatusCode, Value) {
    call(
        app,
        Method::GET,
        &format!("/api/investments/capital-gains?{query}"),
        None,
    )
    .await
}

async fn report(app: &axum::Router, query: &str) -> Value {
    let (status, body) = try_report(app, query).await;
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

#[tokio::test]
async fn a_gift_with_no_price_is_refused_on_both_sides() {
    let (app, db) = household();
    event(&db, "ope-a", "buy", "2025-01-10", "100", "10");
    event(&db, "ope-a", "transfer", "2025-02-01", "-50", "0");
    event(&db, "tomi-a", "transfer", "2025-02-01", "50", "0");
    event(&db, "tomi-a", "sell", "2025-03-01", "40", "30");

    // Reported, Ope's gift was a £500 loss on nothing received, and all of
    // Tomi's £1,200 sale was gain on shares that cost nothing.
    for profile in ["ope", "tomi"] {
        let (status, body) = try_report(&app, &format!("profile_ids={profile}")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{profile}: {body}");
        assert_eq!(body["code"], "unpriced_gift_transfer", "{body}");
        let message = body["error"].as_str().unwrap();
        for detail in ["PLTR", "2025-02-01", "ope", "tomi"] {
            assert!(message.contains(detail), "{detail} missing: {message}");
        }
    }

    // Between spouses no market value is needed, so the same ledger reports.
    marry(&app).await;
    let tomi = report(&app, "profile_ids=tomi").await;
    assert_eq!(money(&tomi["summary"]["total_allowable_costs"]), 400.0);
}

#[tokio::test]
async fn a_gift_out_of_the_reported_years_is_refused_only_where_it_still_counts() {
    let (app, db) = household();
    event(&db, "ope-a", "buy", "2024-01-10", "100", "10");
    event(&db, "ope-a", "transfer", "2024-02-01", "-50", "0");
    event(&db, "tomi-a", "transfer", "2024-02-01", "50", "0");
    event(&db, "ope-a", "sell", "2025-06-01", "10", "20");
    event(&db, "tomi-a", "sell", "2025-06-01", "10", "20");
    let year = "start_date=2025-04-06&end_date=2026-04-05";

    // Ope's later sale does not depend on what the gift was worth.
    let ope = report(&app, &format!("profile_ids=ope&{year}")).await;
    assert_eq!(money(&ope["summary"]["total_allowable_costs"]), 100.0);

    // Tomi's does: the gift is the cost of the shares she sold.
    let (status, body) = try_report(&app, &format!("profile_ids=tomi&{year}")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "unpriced_gift_transfer", "{body}");
}

#[tokio::test]
async fn a_gift_is_valued_once_at_the_senders_price_and_date() {
    let (app, db) = household();
    // The arriving row is two days later at a different rate and carries no
    // price. The receiver's cost must still be the giver's proceeds.
    usd_rates(
        &app,
        &[
            ("2025-01-10", "0.8"),
            ("2025-02-03", "0.75"),
            ("2025-03-01", "0.8"),
        ],
    )
    .await;
    usd_event(&db, "ope-a", "PLTR", ("buy", "2025-01-10"), "100", "10");
    usd_event(
        &db,
        "ope-a",
        "PLTR",
        ("transfer", "2025-02-03"),
        "-100",
        "20",
    );
    usd_event(
        &db,
        "tomi-a",
        "PLTR",
        ("transfer", "2025-02-05"),
        "100",
        "0",
    );
    usd_event(&db, "tomi-a", "PLTR", ("sell", "2025-03-01"), "100", "30");

    let ope = report(&app, "profile_ids=ope").await;
    assert_eq!(ope["realized_events"][0]["gift_to"], "tomi");
    assert_eq!(
        money(&ope["summary"]["total_proceeds"]),
        1500.0,
        "100 x $20 x 0.75"
    );

    let tomi = report(&app, "profile_ids=tomi").await;
    assert_eq!(
        money(&tomi["summary"]["total_allowable_costs"]),
        1500.0,
        "the giver's proceeds, not the arriving row's $0 or its date's rate"
    );
}

#[tokio::test]
async fn a_transfer_arriving_earlier_in_the_day_than_it_left_still_carries_cost() {
    let (app, db) = household();
    marry(&app).await;
    event(&db, "ope-a", "buy", "2025-01-10", "100", "10");
    insert(
        &db,
        CreateInvestmentEventBody {
            date: "2025-02-01T16:00:00".to_string(),
            ..row("ope-a", "transfer", "2025-02-01", "-50", "25")
        },
    );
    insert(
        &db,
        CreateInvestmentEventBody {
            date: "2025-02-01T09:30:00".to_string(),
            ..row("tomi-a", "transfer", "2025-02-01", "50", "25")
        },
    );
    event(&db, "tomi-a", "sell", "2025-03-01", "50", "30");

    let tomi = report(&app, "profile_ids=tomi").await;

    assert_eq!(
        money(&tomi["summary"]["total_allowable_costs"]),
        500.0,
        "50 at Ope's £10, not refused as unmatched"
    );
}

#[tokio::test]
async fn the_receivers_report_needs_nothing_from_the_senders_other_holdings() {
    let (app, db) = household();
    // USD is configured but no rate is stored for Ope's MSFT purchase, so any
    // report that pulls it in is refused for a missing exchange rate.
    usd_rates(&app, &[]).await;
    gift_fifty_to_tomi(&db);
    usd_event(&db, "ope-a", "MSFT", ("buy", "2025-01-15"), "10", "400");

    let (status, body) = try_report(&app, "profile_ids=ope").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "missing_exchange_rates", "{body}");

    let gift = report(&app, "profile_ids=tomi").await;
    assert_eq!(money(&gift["summary"]["total_allowable_costs"]), 1000.0);

    marry(&app).await;
    let spouse = report(&app, "profile_ids=tomi").await;
    assert_eq!(money(&spouse["summary"]["total_allowable_costs"]), 600.0);
}

/// Ope gives Tomi 50 of his 100 shares on `date`, with no price on either row.
fn unpriced_gift(db: &Arc<Mutex<Db>>, date: &str) {
    event(db, "ope-a", "buy", "2023-01-10", "100", "10");
    event(db, "ope-a", "transfer", date, "-50", "0");
    event(db, "tomi-a", "transfer", date, "50", "0");
}

#[tokio::test]
async fn an_unpriced_gift_does_not_refuse_the_givers_pools() {
    let (app, db) = household();
    unpriced_gift(&db, "2024-06-01");

    let (status, pools) = call(
        &app,
        Method::GET,
        "/api/investments/pools?profile_ids=ope",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{pools}");
    assert_eq!(money(&pools[0]["current_shares"]), 50.0);
    assert_eq!(money(&pools[0]["total_allowable_expenditure"]), 500.0);

    // The receiver's pool is built from the gift's value, so hers is refused.
    let (status, body) = call(
        &app,
        Method::GET,
        "/api/investments/pools?profile_ids=tomi",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "unpriced_gift_transfer", "{body}");
}

#[tokio::test]
async fn brought_forward_losses_check_only_gifts_given_in_the_lookback_years() {
    let losses = |app: axum::Router| async move {
        call(
            &app,
            Method::GET,
            "/api/investments/brought-forward-losses?tax_year=2024-25&profile_ids=ope",
            None,
        )
        .await
    };

    // Given in 2024-25 itself, which brings losses in rather than out.
    let (app, db) = household();
    unpriced_gift(&db, "2024-06-01");
    let (status, body) = losses(app).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // Given in 2023-24, a lookback year, where it would be a reported loss.
    let (app, db) = household();
    unpriced_gift(&db, "2023-06-01");
    let (status, body) = losses(app).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "unpriced_gift_transfer", "{body}");
}
