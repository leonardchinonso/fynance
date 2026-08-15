//! Integration tests for the UK Capital Gains Tax (CGT) calculation engine.

use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode};
use fynance::model::{Account, AccountType, CreateInvestmentEventBody};
use fynance::server::build_router;
use fynance::storage::Db;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;
use tower::ServiceExt;

fn test_router() -> (axum::Router, Arc<Mutex<Db>>) {
    let dir = tempdir().unwrap();
    let path = dir.keep().join("cgt_test.db");
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
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn setup_account(db: &Db, id: &str, account_type: AccountType) {
    let name = match account_type {
        AccountType::Investment => "Taxable GIA Brokerage",
        AccountType::InvestmentIsa => "Tax-Free ISA Brokerage",
        AccountType::Pension => "SIPP Pension",
        _ => "Test Account",
    };
    let institution = match account_type {
        AccountType::Investment => "Trading 212",
        AccountType::InvestmentIsa => "Freetrade",
        AccountType::Pension => "Vanguard",
        _ => "Test Institution",
    };

    let account = Account {
        id: id.to_string(),
        name: name.to_string(),
        institution: institution.to_string(),
        account_type,
        currency: "GBP".to_string(),
        balance: None,
        balance_date: None,
        is_active: true,
        notes: None,
        profile_ids: vec!["default".to_string()],
        is_stale: None,
        is_available: true,
    };
    db.create_account(&account).unwrap();
}

#[allow(clippy::too_many_arguments)]
fn insert_event(
    db: &Db,
    account_id: &str,
    event_type: &str,
    symbol: &str,
    date: &str,
    quantity: &str,
    price: &str,
    fee: Option<&str>,
) {
    let body = CreateInvestmentEventBody {
        account_id: account_id.to_string(),
        event_type: event_type.to_string(),
        symbol: symbol.to_string(),
        date: date.to_string(),
        quantity: quantity.to_string(),
        price_per_share: price.to_string(),
        fee: fee.map(|s| s.to_string()),
        currency: "GBP".to_string(),
        fee_currency: None,
        notes: None,
        source_document_ids: Vec::new(),
    };
    db.create_investment_event(&body).unwrap();
}

/// Like `insert_event` but with an explicit trade currency and an optional,
/// possibly-different fee currency. Used by the cross-currency fee tests.
#[allow(clippy::too_many_arguments)]
fn insert_event_ccy(
    db: &Db,
    account_id: &str,
    event_type: &str,
    symbol: &str,
    date: &str,
    quantity: &str,
    price: &str,
    fee: Option<&str>,
    currency: &str,
    fee_currency: Option<&str>,
) {
    let body = CreateInvestmentEventBody {
        account_id: account_id.to_string(),
        event_type: event_type.to_string(),
        symbol: symbol.to_string(),
        date: date.to_string(),
        quantity: quantity.to_string(),
        price_per_share: price.to_string(),
        fee: fee.map(|s| s.to_string()),
        currency: currency.to_string(),
        fee_currency: fee_currency.map(|s| s.to_string()),
        notes: None,
        source_document_ids: Vec::new(),
    };
    db.create_investment_event(&body).unwrap();
}

/// Seed date-keyed exchange rates through the real endpoint.
///
/// Every non-GBP CGT test needs this: the engine converts each leg at its own date's rate and
/// refuses (`missing_exchange_rates`) rather than falling back to the flat `currencies` rate, so
/// a foreign-currency test without stored rates is a 400, not a calculation. Rates are supplied
/// per date deliberately — that is the unit the engine looks up, and it is what makes a test
/// able to detect a rate being read for the wrong date.
async fn seed_rates(app: &axum::Router, base: &str, dates_and_rates: &[(&str, &str)]) {
    let rates: Vec<serde_json::Value> = dates_and_rates
        .iter()
        .map(|(date, rate)| serde_json::json!({ "base": base, "date": date, "rate": rate }))
        .collect();
    let response = app
        .clone()
        .oneshot(request_json(
            Method::POST,
            "/api/exchange-rates",
            serde_json::json!({ "rates": rates }),
        ))
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::CREATED,
        "seeding exchange rates should succeed"
    );
}

#[tokio::test]
async fn test_cgt_same_day_matching() {
    let (app, db) = test_router();
    {
        let db_lock = db.lock().unwrap();
        setup_account(&db_lock, "gia", AccountType::Investment);

        // Same-day: Buy 100 @ 10, Sell 50 @ 15
        insert_event(
            &db_lock,
            "gia",
            "buy",
            "AAPL",
            "2026-05-25T10:00:00",
            "100",
            "10.00",
            None,
        );
        insert_event(
            &db_lock,
            "gia",
            "sell",
            "AAPL",
            "2026-05-25T15:00:00",
            "50",
            "15.00",
            None,
        );
    }

    // Call the /api/investments/capital-gains endpoint for tax year 2026-27
    let response = app
        .oneshot(request(
            Method::GET,
            "/api/investments/capital-gains?start_date=2026-04-06&end_date=2027-04-05",
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let res: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // Check realized events
    let realized = &res["realized_events"];
    assert_eq!(realized.as_array().unwrap().len(), 1);

    let event = &realized[0];
    assert_eq!(event["symbol"], "AAPL");
    assert_eq!(event["quantity"], "50");
    assert_eq!(event["proceeds"], "750.00");
    assert_eq!(event["cost_basis"], "500.00");
    assert_eq!(event["gain_loss"], "250.00");
    assert_eq!(event["rule_applied"], "Same-Day");

    // Check pools
    let pools = &res["pools"];
    let aapl_pool = pools
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["symbol"] == "AAPL")
        .unwrap();
    assert_eq!(aapl_pool["current_shares"], "50"); // 100 - 50 same-day matched
    assert_eq!(aapl_pool["total_allowable_expenditure"], "500.00");
}

#[tokio::test]
async fn test_cgt_30_day_matching() {
    let (app, db) = test_router();
    {
        let db_lock = db.lock().unwrap();
        setup_account(&db_lock, "gia", AccountType::Investment);

        // 30-day B&B: Sell 50 @ 20 on Jun 1st, Buy 50 @ 12 on Jun 15th (matches B&B)
        insert_event(
            &db_lock,
            "gia",
            "sell",
            "AAPL",
            "2026-06-01T10:00:00",
            "50",
            "20.00",
            None,
        );
        insert_event(
            &db_lock,
            "gia",
            "buy",
            "AAPL",
            "2026-06-15T10:00:00",
            "50",
            "12.00",
            None,
        );
    }

    let response = app
        .oneshot(request(
            Method::GET,
            "/api/investments/capital-gains?start_date=2026-04-06&end_date=2027-04-05",
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let res: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let realized = &res["realized_events"];
    assert_eq!(realized.as_array().unwrap().len(), 1);

    let event = &realized[0];
    assert_eq!(event["quantity"], "50");
    assert_eq!(event["proceeds"], "1000.00");
    assert_eq!(event["cost_basis"], "600.00");
    assert_eq!(event["gain_loss"], "400.00");
    assert_eq!(event["rule_applied"], "30-Day Rule");
}

#[tokio::test]
async fn test_cgt_s104_pool_matching() {
    let (app, db) = test_router();
    {
        let db_lock = db.lock().unwrap();
        setup_account(&db_lock, "gia", AccountType::Investment);

        // S104 Pool: Buy 100 @ 10, Buy 100 @ 20 (Average = 15). Sell 100 @ 25.
        insert_event(
            &db_lock,
            "gia",
            "buy",
            "TSLA",
            "2026-05-01T10:00:00",
            "100",
            "10.00",
            None,
        );
        insert_event(
            &db_lock,
            "gia",
            "buy",
            "TSLA",
            "2026-05-10T10:00:00",
            "100",
            "20.00",
            None,
        );
        insert_event(
            &db_lock,
            "gia",
            "sell",
            "TSLA",
            "2026-05-20T10:00:00",
            "100",
            "25.00",
            None,
        );
    }

    let response = app
        .oneshot(request(
            Method::GET,
            "/api/investments/capital-gains?start_date=2026-04-06&end_date=2027-04-05",
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let res: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let realized = &res["realized_events"];
    assert_eq!(realized.as_array().unwrap().len(), 1);

    let event = &realized[0];
    assert_eq!(event["quantity"], "100");
    assert_eq!(event["proceeds"], "2500.00");
    assert_eq!(event["cost_basis"], "1500.00");
    assert_eq!(event["gain_loss"], "1000.00");
    assert_eq!(event["rule_applied"], "S104 Pool");

    let pools = &res["pools"];
    let tsla_pool = pools
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["symbol"] == "TSLA")
        .unwrap();
    assert_eq!(tsla_pool["current_shares"], "100");
    assert_eq!(tsla_pool["total_allowable_expenditure"], "1500.00");
}

#[tokio::test]
async fn test_cgt_stock_split() {
    let (app, db) = test_router();
    {
        let db_lock = db.lock().unwrap();
        setup_account(&db_lock, "gia", AccountType::Investment);

        // Buy 100 MSFT @ 10 (total cost 1000). Split 2-for-1 on May 5th, which ADDS
        // 100 shares (quantity is the shares added, not a ratio), taking the pool to
        // 200 shares at an unchanged cost of 1000, so 5/share. Sell 100 MSFT @ 8.
        insert_event(
            &db_lock,
            "gia",
            "buy",
            "MSFT",
            "2026-05-01T10:00:00",
            "100",
            "10.00",
            None,
        );
        insert_event(
            &db_lock,
            "gia",
            "split",
            "MSFT",
            "2026-05-05T10:00:00",
            "100",
            "0.00",
            None,
        );
        insert_event(
            &db_lock,
            "gia",
            "sell",
            "MSFT",
            "2026-05-10T10:00:00",
            "100",
            "8.00",
            None,
        );
    }

    let response = app
        .oneshot(request(
            Method::GET,
            "/api/investments/capital-gains?start_date=2026-04-06&end_date=2027-04-05",
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let res: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let realized = &res["realized_events"];
    assert_eq!(realized.as_array().unwrap().len(), 1);

    let event = &realized[0];
    assert_eq!(event["quantity"], "100");
    assert_eq!(event["proceeds"], "800.00");
    assert_eq!(event["cost_basis"], "500.00"); // Cost basis per share is 5 after split
    assert_eq!(event["gain_loss"], "300.00");
    assert_eq!(event["rule_applied"], "S104 Pool");

    let pools = &res["pools"];
    let msft_pool = pools
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["symbol"] == "MSFT")
        .unwrap();
    assert_eq!(msft_pool["current_shares"], "100");
    assert_eq!(msft_pool["total_allowable_expenditure"], "500.00");
}

#[tokio::test]
async fn test_cgt_complex_scenario() {
    let (app, db) = test_router();
    {
        let db_lock = db.lock().unwrap();
        setup_account(&db_lock, "gia", AccountType::Investment);

        // 1. Monthly Vests (Months 1-5)
        insert_event(
            &db_lock,
            "gia",
            "vest",
            "GOOG",
            "2026-01-01T09:00:00",
            "10",
            "100.00",
            None,
        );
        insert_event(
            &db_lock,
            "gia",
            "vest",
            "GOOG",
            "2026-02-01T09:00:00",
            "10",
            "110.00",
            None,
        );
        insert_event(
            &db_lock,
            "gia",
            "vest",
            "GOOG",
            "2026-03-01T09:00:00",
            "10",
            "120.00",
            None,
        );
        insert_event(
            &db_lock,
            "gia",
            "vest",
            "GOOG",
            "2026-04-01T09:00:00",
            "10",
            "130.00",
            None,
        );
        insert_event(
            &db_lock,
            "gia",
            "vest",
            "GOOG",
            "2026-05-01T09:00:00",
            "10",
            "140.00",
            None,
        );

        // 2. Stock split of 10-for-1 on May 15th. Quantity is the shares ADDED, so
        // the 50 shares held (5 vests x 10, cost 6000) become 500: 450 added, cost
        // unchanged, giving the 12.00/share S104 average the matches below expect.
        insert_event(
            &db_lock,
            "gia",
            "split",
            "GOOG",
            "2026-05-15T09:00:00",
            "450",
            "0.00",
            None,
        );

        // 3. Same-Day disposal match on June 1st: Sell 50, Buy 50 (same day)
        insert_event(
            &db_lock,
            "gia",
            "sell",
            "GOOG",
            "2026-06-01T10:00:00",
            "50",
            "18.00",
            None,
        );
        insert_event(
            &db_lock,
            "gia",
            "buy",
            "GOOG",
            "2026-06-01T15:00:00",
            "50",
            "16.00",
            None,
        );

        // 4. 30-Day B&B rule match: Sell 100 on July 1st. Buy 50 on July 15th (within 30 days).
        // 50 shares match the July 15th buy; 50 match S104 Pool.
        insert_event(
            &db_lock,
            "gia",
            "sell",
            "GOOG",
            "2026-07-01T09:00:00",
            "100",
            "20.00",
            None,
        );
        insert_event(
            &db_lock,
            "gia",
            "buy",
            "GOOG",
            "2026-07-15T09:00:00",
            "50",
            "15.00",
            None,
        );

        // 5. Pure S104 Pool disposal on August 1st: Sell 100
        insert_event(
            &db_lock,
            "gia",
            "sell",
            "GOOG",
            "2026-08-01T09:00:00",
            "100",
            "25.00",
            None,
        );
    }

    let response = app
        .oneshot(request(
            Method::GET,
            "/api/investments/capital-gains?start_date=2026-04-06&end_date=2027-04-05",
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let res: serde_json::Value = serde_json::from_slice(&body).unwrap();
    println!("DEBUG res = {:#?}", res);

    let realized = res["realized_events"].as_array().unwrap();

    // Assert 4 matches were returned
    assert_eq!(realized.len(), 4);

    // Match 1: Same-Day sale on 2026-06-01
    let m1 = realized
        .iter()
        .find(|e| e["rule_applied"] == "Same-Day")
        .unwrap();
    assert_eq!(m1["quantity"], "50");
    assert_eq!(m1["proceeds"], "900.00");
    assert_eq!(m1["cost_basis"], "800.00");
    assert_eq!(m1["gain_loss"], "100.00");

    // Match 2: 30-Day B&B match on 2026-07-01 (matches the July 15th Buy)
    let m2 = realized
        .iter()
        .find(|e| e["rule_applied"] == "30-Day Rule")
        .unwrap();
    assert_eq!(m2["quantity"], "50");
    assert_eq!(m2["proceeds"], "1000.00");
    assert_eq!(m2["cost_basis"], "750.00");
    assert_eq!(m2["gain_loss"], "250.00");

    // Match 3: S104 Pool match on 2026-07-01 (remaining 50 shares of the July 1st Sale)
    let m3 = realized
        .iter()
        .find(|e| e["rule_applied"] == "S104 Pool" && e["disposal_date"] == "2026-07-01 09:00:00")
        .unwrap();
    assert_eq!(m3["quantity"], "50");
    assert_eq!(m3["proceeds"], "1000.00");
    assert_eq!(m3["cost_basis"], "600.00"); // 50 * 12.00 S104 avg cost
    assert_eq!(m3["gain_loss"], "400.00");

    // Match 4: S104 Pool match on 2026-08-01 (disposal of 100 shares)
    let m4 = realized
        .iter()
        .find(|e| e["rule_applied"] == "S104 Pool" && e["disposal_date"] == "2026-08-01 09:00:00")
        .unwrap();
    assert_eq!(m4["quantity"], "100");
    assert_eq!(m4["proceeds"], "2500.00");
    assert_eq!(m4["cost_basis"], "1200.00"); // 100 * 12.00 S104 avg cost
    assert_eq!(m4["gain_loss"], "1300.00");

    // Check pools
    let pools = &res["pools"];
    let goog_pool = pools
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["symbol"] == "GOOG")
        .unwrap();
    assert_eq!(goog_pool["current_shares"], "350"); // 500 initial - 50 pool matching on July 1st - 100 pool matching on Aug 1st
    assert_eq!(goog_pool["total_allowable_expenditure"], "4200.00"); // 350 * 12.00 S104 avg cost
    assert_eq!(goog_pool["average_cost_per_share"], "12.00");

    // Check summary block
    let summary = &res["summary"];
    assert_eq!(summary["total_proceeds"], "5400.00");
    assert_eq!(summary["total_allowable_costs"], "3350.00");
    assert_eq!(summary["total_gains"], "2050.00");
    assert_eq!(summary["total_losses"], "0");
    assert_eq!(summary["net_gain_loss"], "2050.00");
    assert_eq!(summary["base_currency"], "GBP");
}

#[tokio::test]
async fn test_cgt_tax_sheltered_exclusion() {
    let (app, db) = test_router();
    {
        let db_lock = db.lock().unwrap();
        setup_account(&db_lock, "gia", AccountType::Investment);
        setup_account(&db_lock, "isa", AccountType::InvestmentIsa);
        setup_account(&db_lock, "pension", AccountType::Pension);

        // GIA (Taxable): Buy 100 @ 10, Sell 50 @ 15 -> Taxable CGT proceeds 750, cost 500, gain 250
        insert_event(
            &db_lock,
            "gia",
            "buy",
            "AAPL",
            "2026-05-01T10:00:00",
            "100",
            "10.00",
            None,
        );
        insert_event(
            &db_lock,
            "gia",
            "sell",
            "AAPL",
            "2026-05-10T10:00:00",
            "50",
            "15.00",
            None,
        );

        // ISA (Tax-Sheltered): Buy 100 @ 10, Sell 50 @ 20 -> Ignored completely
        insert_event(
            &db_lock,
            "isa",
            "buy",
            "AAPL",
            "2026-05-01T10:00:00",
            "100",
            "10.00",
            None,
        );
        insert_event(
            &db_lock,
            "isa",
            "sell",
            "AAPL",
            "2026-05-10T10:00:00",
            "50",
            "20.00",
            None,
        );

        // Pension (Tax-Sheltered): Buy 100 @ 10, Sell 50 @ 30 -> Ignored completely
        insert_event(
            &db_lock,
            "pension",
            "buy",
            "AAPL",
            "2026-05-01T10:00:00",
            "100",
            "10.00",
            None,
        );
        insert_event(
            &db_lock,
            "pension",
            "sell",
            "AAPL",
            "2026-05-10T10:00:00",
            "50",
            "30.00",
            None,
        );
    }

    let response = app
        .oneshot(request(
            Method::GET,
            "/api/investments/capital-gains?start_date=2026-04-06&end_date=2027-04-05",
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let res: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // Verify realized events has strictly 1 element (from GIA)
    let realized = res["realized_events"].as_array().unwrap();
    assert_eq!(realized.len(), 1);

    let event = &realized[0];
    assert_eq!(event["symbol"], "AAPL");
    assert_eq!(event["quantity"], "50");
    assert_eq!(event["proceeds"], "750.00");
    assert_eq!(event["cost_basis"], "500.00");
    assert_eq!(event["gain_loss"], "250.00");

    // Verify S104 Pool state tracks only the GIA transactions
    let pools = res["pools"].as_array().unwrap();
    let aapl_pool = pools.iter().find(|p| p["symbol"] == "AAPL").unwrap();
    assert_eq!(aapl_pool["current_shares"], "50");
    assert_eq!(aapl_pool["total_allowable_expenditure"], "500.00");
    assert_eq!(aapl_pool["average_cost_per_share"], "10.00");
}

#[tokio::test]
async fn test_cgt_transaction_fees() {
    let (app, db) = test_router();
    {
        let db_lock = db.lock().unwrap();
        setup_account(&db_lock, "gia", AccountType::Investment);

        // Buy 100 AAPL @ 10.00 with a 20.00 fee -> Total expenditure = 1000 + 20 = 1020 (Avg cost = 10.20)
        insert_event(
            &db_lock,
            "gia",
            "buy",
            "AAPL",
            "2026-05-01T10:00:00",
            "100",
            "10.00",
            Some("20.00"),
        );

        // Sell 50 AAPL @ 15.00 with a 15.00 fee -> Net proceeds = 750 - 15 = 735
        // Proportional cost basis matched = 50 * 10.20 = 510
        // Net gain = 735 - 510 = 225
        insert_event(
            &db_lock,
            "gia",
            "sell",
            "AAPL",
            "2026-05-10T10:00:00",
            "50",
            "15.00",
            Some("15.00"),
        );
    }

    let response = app
        .oneshot(request(
            Method::GET,
            "/api/investments/capital-gains?start_date=2026-04-06&end_date=2027-04-05",
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let res: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let realized = res["realized_events"].as_array().unwrap();
    assert_eq!(realized.len(), 1);

    let event = &realized[0];
    assert_eq!(event["quantity"], "50");
    assert_eq!(event["proceeds"], "735.00");
    assert_eq!(event["cost_basis"], "510.00");
    assert_eq!(event["gain_loss"], "225.00");
    assert_eq!(event["rule_applied"], "S104 Pool");

    let pools = res["pools"].as_array().unwrap();
    let aapl_pool = pools.iter().find(|p| p["symbol"] == "AAPL").unwrap();
    assert_eq!(aapl_pool["current_shares"], "50");
    assert_eq!(aapl_pool["total_allowable_expenditure"], "510.00");
    assert_eq!(aapl_pool["average_cost_per_share"], "10.20");
}

#[tokio::test]
/// A disposal with no matching acquisition is REFUSED, not reported.
///
/// This test previously asserted the opposite — a row with `cost_basis = 0`,
/// `rule_applied = "Unmatched"` and the full proceeds as gain. That behaviour
/// overstates the tax due and looks like an ordinary line on the report, so it
/// was deliberately changed to a refusal (plan 23 §0.2).
async fn test_cgt_short_sales_unmatched() {
    let (app, db) = test_router();
    {
        let db_lock = db.lock().unwrap();
        setup_account(&db_lock, "gia", AccountType::Investment);

        // Sell 100 AAPL @ 15.00 with no acquisitions anywhere in the ledger.
        insert_event(
            &db_lock,
            "gia",
            "sell",
            "AAPL",
            "2026-05-10T10:00:00",
            "100",
            "15.00",
            None,
        );
    }

    let response = app
        .oneshot(request(
            Method::GET,
            "/api/investments/capital-gains?start_date=2026-04-06&end_date=2027-04-05",
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let res: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(res["code"], "unmatched_disposal");
    let msg = res["error"].as_str().unwrap();
    // The message must let the user find the missing acquisition: symbol, date, quantity.
    assert!(msg.contains("AAPL"), "message must name the symbol: {msg}");
    assert!(
        msg.contains("2026-05-10"),
        "message must name the date: {msg}"
    );
    assert!(msg.contains("100"), "message must name the quantity: {msg}");
    // And it must say what to do about it, not merely that something is wrong.
    assert!(
        msg.contains("Import or add the missing acquisition"),
        "message must say what to do next: {msg}"
    );
}

/// A disposal only PARTLY covered by the pool is refused too — the uncovered
/// remainder is the same zero-cost problem, just harder to spot next to a
/// legitimate matched row.
#[tokio::test]
async fn test_cgt_partially_unmatched_disposal_is_refused() {
    let (app, db) = test_router();
    {
        let db_lock = db.lock().unwrap();
        setup_account(&db_lock, "gia", AccountType::Investment);

        // Buy 40, then sell 100 — 60 shares have no acquisition behind them.
        insert_event(
            &db_lock,
            "gia",
            "buy",
            "AAPL",
            "2026-04-10T10:00:00",
            "40",
            "10.00",
            None,
        );
        insert_event(
            &db_lock,
            "gia",
            "sell",
            "AAPL",
            "2026-05-10T10:00:00",
            "100",
            "15.00",
            None,
        );
    }

    let response = app
        .oneshot(request(
            Method::GET,
            "/api/investments/capital-gains?start_date=2026-04-06&end_date=2027-04-05",
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let res: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(res["code"], "unmatched_disposal");
    // 60 unmatched, not the full 100 — the pool covered 40.
    assert!(
        res["error"].as_str().unwrap().contains("60"),
        "message should name the unmatched quantity: {}",
        res["error"]
    );
}

/// The counterpart that matters more: a fully-matched disposal is NOT refused.
/// An over-eager unmatched guard would block every legitimate report.
#[tokio::test]
async fn test_cgt_fully_matched_disposal_is_not_refused() {
    let (app, db) = test_router();
    {
        let db_lock = db.lock().unwrap();
        setup_account(&db_lock, "gia", AccountType::Investment);

        insert_event(
            &db_lock,
            "gia",
            "buy",
            "AAPL",
            "2026-04-10T10:00:00",
            "100",
            "10.00",
            None,
        );
        insert_event(
            &db_lock,
            "gia",
            "sell",
            "AAPL",
            "2026-05-10T10:00:00",
            "100",
            "15.00",
            None,
        );
    }

    let response = app
        .oneshot(request(
            Method::GET,
            "/api/investments/capital-gains?start_date=2026-04-06&end_date=2027-04-05",
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let res: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let realized = res["realized_events"].as_array().unwrap();
    assert_eq!(realized.len(), 1);
    assert_eq!(realized[0]["rule_applied"], "S104 Pool");
    assert_eq!(realized[0]["cost_basis"], "1000.00");
    assert_eq!(realized[0]["gain_loss"], "500.00");
}

#[tokio::test]
async fn test_cgt_multi_account_pooling() {
    let (app, db) = test_router();
    {
        let db_lock = db.lock().unwrap();
        setup_account(&db_lock, "gia_1", AccountType::Investment);

        let account = Account {
            id: "gia_2".to_string(),
            name: "GIA Brokerage 2".to_string(),
            institution: "Freetrade".to_string(),
            account_type: AccountType::Investment,
            currency: "GBP".to_string(),
            balance: None,
            balance_date: None,
            is_active: true,
            notes: None,
            profile_ids: vec!["default".to_string()],
            is_stale: None,
            is_available: true,
        };
        db_lock.create_account(&account).unwrap();

        // gia_1: Buy 50 AAPL @ 10 -> allowable expenditure = 500
        insert_event(
            &db_lock,
            "gia_1",
            "buy",
            "AAPL",
            "2026-05-01T10:00:00",
            "50",
            "10.00",
            None,
        );

        // gia_2: Buy 50 AAPL @ 20 -> allowable expenditure = 1000
        // Global symbol S104 pool AAPL = 100 shares at 1500 total cost (Avg cost = 15.00)
        insert_event(
            &db_lock,
            "gia_2",
            "buy",
            "AAPL",
            "2026-05-05T10:00:00",
            "50",
            "20.00",
            None,
        );

        // gia_1: Sell 50 AAPL @ 25 -> Proceeds = 1250. Cost matched = 50 * 15 = 750. Gain = 500
        insert_event(
            &db_lock,
            "gia_1",
            "sell",
            "AAPL",
            "2026-05-10T10:00:00",
            "50",
            "25.00",
            None,
        );
    }

    let response = app
        .oneshot(request(
            Method::GET,
            "/api/investments/capital-gains?start_date=2026-04-06&end_date=2027-04-05",
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let res: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let realized = res["realized_events"].as_array().unwrap();
    assert_eq!(realized.len(), 1);

    let event = &realized[0];
    assert_eq!(event["quantity"], "50");
    assert_eq!(event["proceeds"], "1250.00");
    assert_eq!(event["cost_basis"], "750.00");
    assert_eq!(event["gain_loss"], "500.00");
    assert_eq!(event["rule_applied"], "S104 Pool");

    let pools = res["pools"].as_array().unwrap();
    let aapl_pool = pools.iter().find(|p| p["symbol"] == "AAPL").unwrap();
    assert_eq!(aapl_pool["current_shares"], "50");
    assert_eq!(aapl_pool["total_allowable_expenditure"], "750.00");
    assert_eq!(aapl_pool["average_cost_per_share"], "15.00");
}

/// Covers the collapsed time-window contract (plan 23 §0.2, decision 7.3):
/// `/api/investments/pools?end_date=` still truncates the *ledger* (a genuine
/// point-in-time snapshot — there is nothing to emit-filter for pool state),
/// while `/api/investments/capital-gains` no longer truncates the ledger at
/// all — `end_date` there only filters which disposals are *emitted*, and
/// the S104 pool embedded in that response always reflects the full replay.
/// That is the whole point of dropping `as_at`: the old code let the same
/// disposal get a different cost basis depending on which of the two
/// interchangeable-sounding params the caller used.
#[tokio::test]
async fn test_cgt_point_in_time_filtering() {
    let (app, db) = test_router();
    {
        let db_lock = db.lock().unwrap();
        setup_account(&db_lock, "gia", AccountType::Investment);

        // Buy 100 AAPL @ 10 on May 1st -> allowable expenditure = 1000
        insert_event(
            &db_lock,
            "gia",
            "buy",
            "AAPL",
            "2026-05-01T10:00:00",
            "100",
            "10.00",
            None,
        );

        // Buy 100 AAPL @ 20 on May 10th -> allowable expenditure = 2000
        insert_event(
            &db_lock,
            "gia",
            "buy",
            "AAPL",
            "2026-05-10T10:00:00",
            "100",
            "20.00",
            None,
        );

        // Sell 100 AAPL @ 25 on May 20th
        insert_event(
            &db_lock,
            "gia",
            "sell",
            "AAPL",
            "2026-05-20T10:00:00",
            "100",
            "25.00",
            None,
        );
    }

    // 1. `/api/investments/pools?end_date=2026-05-05` — still a ledger truncation.
    // Only the first buy (100 shares @ 10) has happened by this date; the second
    // buy and the sale are both entirely excluded from the replay, not merely
    // hidden from the output.
    let response_pools = app
        .clone()
        .oneshot(request(
            Method::GET,
            "/api/investments/pools?end_date=2026-05-05",
        ))
        .await
        .unwrap();

    assert_eq!(response_pools.status(), StatusCode::OK);
    let body_pools = to_bytes(response_pools.into_body(), usize::MAX)
        .await
        .unwrap();
    let res_pools: serde_json::Value = serde_json::from_slice(&body_pools).unwrap();

    let aapl_pool_pit = res_pools
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["symbol"] == "AAPL")
        .unwrap();
    assert_eq!(aapl_pool_pit["current_shares"], "100");
    assert_eq!(aapl_pool_pit["total_allowable_expenditure"], "1000.00");
    assert_eq!(aapl_pool_pit["average_cost_per_share"], "10.00");

    // 2. `/api/investments/capital-gains?end_date=2026-05-05` (no `start_date`) —
    // absent `start_date` means "from time zero" (decision 7.3), so this is the
    // "as at a date" report use case. Unlike (1), the ledger is NOT truncated:
    // the replay still runs the May 20th sale against the pool (average cost 15,
    // pool ends at 100 shares / 1500 expenditure — same final state as an
    // unfiltered query), even though that sale is excluded from `realized_events`
    // because its date falls after `end_date`. This is precisely the divergence
    // decision 7.3 removes: the old `as_at` would have truncated the ledger here
    // and left the pool at 100 shares @ cost 10 instead.
    let response_asat = app
        .clone()
        .oneshot(request(
            Method::GET,
            "/api/investments/capital-gains?end_date=2026-05-05",
        ))
        .await
        .unwrap();

    assert_eq!(response_asat.status(), StatusCode::OK);
    let body_asat = to_bytes(response_asat.into_body(), usize::MAX)
        .await
        .unwrap();
    let res_asat: serde_json::Value = serde_json::from_slice(&body_asat).unwrap();

    assert_eq!(res_asat["realized_events"].as_array().unwrap().len(), 0);

    let aapl_pool_asat = res_asat["pools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["symbol"] == "AAPL")
        .unwrap();
    assert_eq!(aapl_pool_asat["current_shares"], "100");
    assert_eq!(aapl_pool_asat["total_allowable_expenditure"], "1500.00");
    assert_eq!(aapl_pool_asat["average_cost_per_share"], "15.00");

    // 3. Query custom date range `start_date=2026-05-15&end_date=2026-05-25`
    // Realized events contains strictly the sale on May 20th. (Proceeds 2500, S104 pool average cost 15.00, Matched Cost 1500, Gain 1000)
    let response_range = app
        .oneshot(request(
            Method::GET,
            "/api/investments/capital-gains?start_date=2026-05-15&end_date=2026-05-25",
        ))
        .await
        .unwrap();

    assert_eq!(response_range.status(), StatusCode::OK);
    let body_range = to_bytes(response_range.into_body(), usize::MAX)
        .await
        .unwrap();
    let res_range: serde_json::Value = serde_json::from_slice(&body_range).unwrap();

    let realized_range = res_range["realized_events"].as_array().unwrap();
    assert_eq!(realized_range.len(), 1);

    let event_range = &realized_range[0];
    assert_eq!(event_range["quantity"], "100");
    assert_eq!(event_range["proceeds"], "2500.00");
    assert_eq!(event_range["cost_basis"], "1500.00");
    assert_eq!(event_range["gain_loss"], "1000.00");
    assert_eq!(event_range["rule_applied"], "S104 Pool");
}

/// Two GIA accounts on different profiles, each with its own AAPL disposal.
/// `?profile_ids=alice` must return only Alice's disposal; the S104 pool is
/// scoped to her events too (the engine should not see Bob's at all).
#[tokio::test]
async fn test_cgt_profile_ids_filter() {
    let (app, db) = test_router();
    {
        let db_lock = db.lock().unwrap();

        let alice = Account {
            id: "gia_alice".to_string(),
            name: "Alice GIA".to_string(),
            institution: "Trading 212".to_string(),
            account_type: AccountType::Investment,
            currency: "GBP".to_string(),
            balance: None,
            balance_date: None,
            is_active: true,
            notes: None,
            profile_ids: vec!["alice".to_string()],
            is_stale: None,
            is_available: true,
        };
        let bob = Account {
            id: "gia_bob".to_string(),
            name: "Bob GIA".to_string(),
            institution: "Freetrade".to_string(),
            account_type: AccountType::Investment,
            currency: "GBP".to_string(),
            balance: None,
            balance_date: None,
            is_active: true,
            notes: None,
            profile_ids: vec!["bob".to_string()],
            is_stale: None,
            is_available: true,
        };
        db_lock.create_account(&alice).unwrap();
        db_lock.create_account(&bob).unwrap();

        // Alice: Buy 100 @ 10, Sell 100 @ 20 → £1,000 gain on AAPL
        insert_event(
            &db_lock,
            "gia_alice",
            "buy",
            "AAPL",
            "2026-05-01T10:00:00",
            "100",
            "10.00",
            None,
        );
        insert_event(
            &db_lock,
            "gia_alice",
            "sell",
            "AAPL",
            "2026-06-01T10:00:00",
            "100",
            "20.00",
            None,
        );

        // Bob: Buy 50 @ 30, Sell 50 @ 25 → £250 loss on AAPL
        insert_event(
            &db_lock,
            "gia_bob",
            "buy",
            "AAPL",
            "2026-05-02T10:00:00",
            "50",
            "30.00",
            None,
        );
        insert_event(
            &db_lock,
            "gia_bob",
            "sell",
            "AAPL",
            "2026-06-02T10:00:00",
            "50",
            "25.00",
            None,
        );
    }

    // Filter to Alice only.
    let resp_alice = app
        .clone()
        .oneshot(request(
            Method::GET,
            "/api/investments/capital-gains?start_date=2026-04-06&end_date=2027-04-05&profile_ids=alice",
        ))
        .await
        .unwrap();
    assert_eq!(resp_alice.status(), StatusCode::OK);
    let body_alice = to_bytes(resp_alice.into_body(), usize::MAX).await.unwrap();
    let res_alice: serde_json::Value = serde_json::from_slice(&body_alice).unwrap();

    let realized_alice = res_alice["realized_events"].as_array().unwrap();
    assert_eq!(realized_alice.len(), 1, "alice scope returns one disposal");
    assert!(
        !realized_alice[0]["disposal_id"]
            .as_str()
            .unwrap()
            .is_empty()
    );
    assert_eq!(realized_alice[0]["proceeds"], "2000.00");
    assert_eq!(realized_alice[0]["cost_basis"], "1000.00");
    assert_eq!(realized_alice[0]["gain_loss"], "1000.00");

    let pools_alice = res_alice["pools"].as_array().unwrap();
    assert_eq!(pools_alice.len(), 1, "alice pool count");
    assert_eq!(pools_alice[0]["symbol"], "AAPL");
    assert_eq!(
        pools_alice[0]["current_shares"], "0",
        "alice's AAPL pool is empty after the sell"
    );

    // Filter to Bob only.
    let resp_bob = app
        .clone()
        .oneshot(request(
            Method::GET,
            "/api/investments/capital-gains?start_date=2026-04-06&end_date=2027-04-05&profile_ids=bob",
        ))
        .await
        .unwrap();
    assert_eq!(resp_bob.status(), StatusCode::OK);
    let body_bob = to_bytes(resp_bob.into_body(), usize::MAX).await.unwrap();
    let res_bob: serde_json::Value = serde_json::from_slice(&body_bob).unwrap();
    let realized_bob = res_bob["realized_events"].as_array().unwrap();
    assert_eq!(realized_bob.len(), 1, "bob scope returns one disposal");
    assert_eq!(realized_bob[0]["proceeds"], "1250.00");
    assert_eq!(realized_bob[0]["cost_basis"], "1500.00");
    assert_eq!(realized_bob[0]["gain_loss"], "-250.00");

    // No filter: both disposals appear.
    let resp_all = app
        .oneshot(request(
            Method::GET,
            "/api/investments/capital-gains?start_date=2026-04-06&end_date=2027-04-05",
        ))
        .await
        .unwrap();
    assert_eq!(resp_all.status(), StatusCode::OK);
    let body_all = to_bytes(resp_all.into_body(), usize::MAX).await.unwrap();
    let res_all: serde_json::Value = serde_json::from_slice(&body_all).unwrap();
    assert_eq!(
        res_all["realized_events"].as_array().unwrap().len(),
        2,
        "unscoped query returns both disposals"
    );
}

/// An event priced in a currency that isn't configured under
/// Settings → Currencies must produce a structured 400 the frontend can show,
/// not a panic that poisons the DB mutex.
#[tokio::test]
async fn test_cgt_missing_currency_returns_400() {
    let (app, db) = test_router();
    {
        let db_lock = db.lock().unwrap();
        setup_account(&db_lock, "gia", AccountType::Investment);
        // Same-day buy + sell, both priced in ZAR (not seeded).
        let body_buy = CreateInvestmentEventBody {
            account_id: "gia".to_string(),
            event_type: "buy".to_string(),
            symbol: "AAPL".to_string(),
            date: "2026-05-25T10:00:00".to_string(),
            quantity: "10".to_string(),
            price_per_share: "100".to_string(),
            fee: None,
            currency: "ZAR".to_string(),
            fee_currency: None,
            notes: None,
            source_document_ids: Vec::new(),
        };
        let body_sell = CreateInvestmentEventBody {
            account_id: "gia".to_string(),
            event_type: "sell".to_string(),
            symbol: "AAPL".to_string(),
            date: "2026-05-25T15:00:00".to_string(),
            quantity: "10".to_string(),
            price_per_share: "150".to_string(),
            fee: None,
            currency: "ZAR".to_string(),
            fee_currency: None,
            notes: None,
            source_document_ids: Vec::new(),
        };
        db_lock.create_investment_event(&body_buy).unwrap();
        db_lock.create_investment_event(&body_sell).unwrap();
    }

    let response = app
        .oneshot(request(
            Method::GET,
            "/api/investments/capital-gains?start_date=2026-04-06&end_date=2027-04-05",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let res: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(res["code"], "missing_currencies");
    assert!(
        res["error"].as_str().unwrap().contains("ZAR"),
        "error message should name the missing currency"
    );
}

/// `profile_ids` matching no accounts is a hard scope: zero events, zero pools.
#[tokio::test]
async fn test_cgt_profile_ids_no_match() {
    let (app, db) = test_router();
    {
        let db_lock = db.lock().unwrap();
        setup_account(&db_lock, "gia", AccountType::Investment);
        insert_event(
            &db_lock,
            "gia",
            "buy",
            "AAPL",
            "2026-05-01T10:00:00",
            "100",
            "10.00",
            None,
        );
        insert_event(
            &db_lock,
            "gia",
            "sell",
            "AAPL",
            "2026-06-01T10:00:00",
            "100",
            "15.00",
            None,
        );
    }

    let resp = app
        .oneshot(request(
            Method::GET,
            "/api/investments/capital-gains?start_date=2026-04-06&end_date=2027-04-05&profile_ids=ghost",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let res: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(res["realized_events"].as_array().unwrap().is_empty());
    assert!(res["pools"].as_array().unwrap().is_empty());
}

/// A fee charged in a different currency from the trade price must be converted
/// at its own rate, not the trade-currency rate. Price in USD, commission in GBP.
#[tokio::test]
async fn test_cgt_fee_in_different_currency() {
    let (app, db) = test_router();
    {
        let db_lock = db.lock().unwrap();
        setup_account(&db_lock, "gia", AccountType::Investment);

        // Buy 100 AAPL @ $10.00 USD, commission £30.00 GBP.
        insert_event_ccy(
            &db_lock,
            "gia",
            "buy",
            "AAPL",
            "2026-05-01T10:00:00",
            "100",
            "10.00",
            Some("30.00"),
            "USD",
            Some("GBP"),
        );
        // Sell 50 AAPL @ $15.00 USD, commission £5.00 GBP.
        insert_event_ccy(
            &db_lock,
            "gia",
            "sell",
            "AAPL",
            "2026-05-10T10:00:00",
            "50",
            "15.00",
            Some("5.00"),
            "USD",
            Some("GBP"),
        );
    }

    // USD = 2 GBP (preferred GBP stays 1). A whole-number rate keeps decimal scale clean.
    let create = app
        .clone()
        .oneshot(request_json(
            Method::POST,
            "/api/currencies",
            serde_json::json!({ "code": "USD", "fx_rate": "2" }),
        ))
        .await
        .unwrap();
    assert_eq!(create.status(), StatusCode::CREATED);

    // The engine converts each leg at its own date. Both dates carry the same rate (2) here so
    // the arithmetic below stays the flat-rate arithmetic this test was written to check — the
    // point of this test is the fee currency, not the date, and holding the rate constant keeps
    // the two concerns from being tangled. `test_cgt_uses_date_specific_rates` is where a
    // differing rate per date is asserted.
    seed_rates(&app, "USD", &[("2026-05-01", "2"), ("2026-05-10", "2")]).await;

    let response = app
        .oneshot(request(
            Method::GET,
            "/api/investments/capital-gains?start_date=2026-04-06&end_date=2027-04-05",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let res: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // Pool cost = price 1000 USD -> 2000 GBP, plus fee 30 GBP (unscaled) = 2030.
    // The buggy behaviour would convert (1000 + 30) USD * 2 = 2060 GBP.
    // After selling 50 of 100 at avg 20.30: remaining expenditure = 1015.00.
    let pools = res["pools"].as_array().unwrap();
    let aapl_pool = pools.iter().find(|p| p["symbol"] == "AAPL").unwrap();
    assert_eq!(aapl_pool["current_shares"], "50");
    assert_eq!(aapl_pool["total_allowable_expenditure"], "1015.00");
    assert_eq!(aapl_pool["average_cost_per_share"], "20.30");

    // Disposal: proceeds 750 USD -> 1500 GBP, less fee 5 GBP = 1495.
    // Cost basis matched = 50 * 20.30 = 1015. Gain = 1495 - 1015 = 480.
    let realized = res["realized_events"].as_array().unwrap();
    assert_eq!(realized.len(), 1);
    let event = &realized[0];
    assert_eq!(event["proceeds"], "1495.00");
    assert_eq!(event["cost_basis"], "1015.00");
    assert_eq!(event["gain_loss"], "480.00");
}

/// A non-zero fee in an unconfigured currency must surface the same structured
/// 400 as an unconfigured price currency, even when the price currency is fine.
#[tokio::test]
async fn test_cgt_missing_fee_currency_returns_400() {
    let (app, db) = test_router();
    {
        let db_lock = db.lock().unwrap();
        setup_account(&db_lock, "gia", AccountType::Investment);
        // Price in GBP (configured); fee in ZAR (not configured).
        insert_event_ccy(
            &db_lock,
            "gia",
            "buy",
            "AAPL",
            "2026-05-01T10:00:00",
            "10",
            "100",
            Some("3.50"),
            "GBP",
            Some("ZAR"),
        );
    }

    let response = app
        .oneshot(request(
            Method::GET,
            "/api/investments/capital-gains?start_date=2026-04-06&end_date=2027-04-05",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let res: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(res["code"], "missing_currencies");
    assert!(
        res["error"].as_str().unwrap().contains("ZAR"),
        "error message should name the missing fee currency"
    );
}

/// A 10-for-1 split of a fractional holding, mirroring a real broker statement:
/// 1.72827619 shares become 17.2827619, so `quantity` is the 15.55448571 shares
/// ADDED. Treating that number as a ratio would inflate the pool to 25.17 shares,
/// understating average cost and overstating every later gain.
#[tokio::test]
async fn test_cgt_split_quantity_is_shares_added_not_a_ratio() {
    let (app, db) = test_router();
    {
        let db_lock = db.lock().unwrap();
        setup_account(&db_lock, "gia", AccountType::Investment);

        // Buy 1.72827619 @ 1000 (cost 1728.27619).
        insert_event(
            &db_lock,
            "gia",
            "buy",
            "NVDA",
            "2026-05-01T10:00:00",
            "1.72827619",
            "1000.00",
            None,
        );
        insert_event(
            &db_lock,
            "gia",
            "split",
            "NVDA",
            "2026-06-10T10:00:00",
            "15.55448571",
            "0.00",
            None,
        );
    }

    let response = app
        .oneshot(request(
            Method::GET,
            "/api/investments/capital-gains?start_date=2026-04-06&end_date=2027-04-05",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let res: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let pool = res["pools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["symbol"] == "NVDA")
        .expect("NVDA pool");

    // Shares added, not multiplied: 1.72827619 + 15.55448571 = 17.2827619.
    assert_eq!(pool["current_shares"], "17.28276190");
    // A split is a reorganisation: total cost is untouched, only the per-share
    // average falls (1000 -> 100, exactly the 10-for-1 ratio).
    assert_eq!(pool["total_allowable_expenditure"], "1728.2761900000");
    let avg: f64 = pool["average_cost_per_share"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!((avg - 100.0).abs() < 1e-9, "average cost per share: {avg}");
}

/// A 1-for-5 consolidation (reverse split) on 100 shares leaves 20, so `quantity`
/// is the -80 shares REMOVED. The engine used to require `quantity > 0` here, which
/// dropped consolidations silently: the pool kept all 100 shares, understating
/// average cost per share and overstating the gain on every later disposal.
#[tokio::test]
async fn test_cgt_consolidation_removes_shares_and_preserves_cost() {
    let (app, db) = test_router();
    {
        let db_lock = db.lock().unwrap();
        setup_account(&db_lock, "gia", AccountType::Investment);

        // Buy 100 @ 10 (cost 1000).
        insert_event(
            &db_lock,
            "gia",
            "buy",
            "TSCO",
            "2026-05-01T10:00:00",
            "100",
            "10.00",
            None,
        );
        // 1-for-5 consolidation: 100 shares become 20, so 80 are removed.
        insert_event(
            &db_lock,
            "gia",
            "split",
            "TSCO",
            "2026-06-10T10:00:00",
            "-80",
            "0.00",
            None,
        );
    }

    let response = app
        .oneshot(request(
            Method::GET,
            "/api/investments/capital-gains?start_date=2026-04-06&end_date=2027-04-05",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let res: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let pool = res["pools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["symbol"] == "TSCO")
        .expect("TSCO pool");

    // Shares removed, not divided: 100 - 80 = 20.
    assert_eq!(pool["current_shares"], "20");
    // A consolidation is a reorganisation too: total cost is untouched, so the
    // per-share average RISES by exactly the 1-for-5 ratio (10 -> 50).
    assert_eq!(pool["total_allowable_expenditure"], "1000.00");
    let avg: f64 = pool["average_cost_per_share"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!((avg - 50.0).abs() < 1e-9, "average cost per share: {avg}");
    // original_currency is source metadata (the symbol's own trade currency), not a
    // label for total_allowable_expenditure / average_cost_per_share above — those
    // are always base currency (GBP).
    assert_eq!(pool["original_currency"], "GBP");
}

/// A consolidation that removes more shares than the pool holds is impossible data,
/// so the engine refuses instead of absorbing it. Clamping at zero would leave a pool
/// with no shares but non-zero cost, making average cost zero and reporting 100% of
/// every later disposal's proceeds as gain — overstating tax while looking ordinary.
#[tokio::test]
async fn test_cgt_consolidation_exceeding_pool_is_refused() {
    let (app, db) = test_router();
    {
        let db_lock = db.lock().unwrap();
        setup_account(&db_lock, "gia", AccountType::Investment);

        // Pool holds 50 shares...
        insert_event(
            &db_lock,
            "gia",
            "buy",
            "TSCO",
            "2026-05-01T10:00:00",
            "50",
            "10.00",
            None,
        );
        // ...but the consolidation claims to remove 80.
        insert_event(
            &db_lock,
            "gia",
            "split",
            "TSCO",
            "2026-06-10T10:00:00",
            "-80",
            "0.00",
            None,
        );
    }

    let response = app
        .oneshot(request(
            Method::GET,
            "/api/investments/capital-gains?start_date=2026-04-06&end_date=2027-04-05",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let res: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(res["code"], "consolidation_exceeds_pool");
    // The message must name the symbol so the bad row can actually be found.
    let msg = res["error"].as_str().unwrap_or_default();
    assert!(
        msg.contains("TSCO"),
        "message should name the symbol: {msg}"
    );
}

/// A consolidation that removes EXACTLY every share the pool holds must clear
/// pool_cost, the same way a Sell that empties the pool does. Left unhandled this
/// orphans the cost: the pool sits at zero shares with non-zero cost, so the next
/// Buy inherits that stale cost into its average, and a later Sell reports far too
/// much allowable expenditure — understating the gain (or manufacturing a loss)
/// on a disposal that never earned that cost.
///
/// This asserts the downstream tax outcome, not just the pool's post-consolidation
/// state: buy 100 @ £10 (cost £1000), consolidate all 100 away, buy 10 @ £20 (cost
/// £200), sell 10 @ £30 (proceeds £300). If the £1000 leaked through, the average
/// cost per share would be computed over the orphaned + new cost and the sale would
/// report a loss instead of the correct £100 gain (£300 - £200).
#[tokio::test]
async fn test_cgt_consolidation_to_exactly_zero_clears_orphaned_cost() {
    let (app, db) = test_router();
    {
        let db_lock = db.lock().unwrap();
        setup_account(&db_lock, "gia", AccountType::Investment);

        // Buy 100 @ 10 (cost 1000).
        insert_event(
            &db_lock,
            "gia",
            "buy",
            "TSCO",
            "2026-05-01T10:00:00",
            "100",
            "10.00",
            None,
        );
        // Consolidation removes exactly the 100 shares the pool holds.
        insert_event(
            &db_lock,
            "gia",
            "split",
            "TSCO",
            "2026-05-10T10:00:00",
            "-100",
            "0.00",
            None,
        );
        // Fresh acquisition into the now-empty pool: 10 @ 20 (cost 200).
        insert_event(
            &db_lock,
            "gia",
            "buy",
            "TSCO",
            "2026-06-01T10:00:00",
            "10",
            "20.00",
            None,
        );
        // Sell all 10 @ 30 (proceeds 300).
        insert_event(
            &db_lock,
            "gia",
            "sell",
            "TSCO",
            "2026-06-15T10:00:00",
            "10",
            "30.00",
            None,
        );
    }

    let response = app
        .oneshot(request(
            Method::GET,
            "/api/investments/capital-gains?start_date=2026-04-06&end_date=2027-04-05",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let res: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let pool = res["pools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["symbol"] == "TSCO")
        .expect("TSCO pool");
    // Pool state alone: all 10 shares were sold, so 0 remain and cost is 0 —
    // not the orphaned 1000 left dangling on an empty pool.
    assert_eq!(pool["current_shares"], "0");
    assert_eq!(pool["total_allowable_expenditure"], "0");

    let disposal = res["realized_events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["symbol"] == "TSCO")
        .expect("TSCO disposal");
    // The downstream consequence: correct £100 gain (300 proceeds - 200 cost).
    // Orphaned cost would report a £900 loss instead (300 - 1200).
    assert_eq!(disposal["cost_basis"], "200.00");
    assert_eq!(disposal["gain_loss"], "100.00");
}

/// A pool holding a non-GBP symbol reports that symbol's own currency as source
/// metadata (`original_currency`), but the pool's cost/average figures are always
/// converted to base currency (GBP) — `original_currency` must NOT be read as a
/// format label for them. This asserts the amount alongside the currency string:
/// a version that swapped the two (labelled the value USD but left it as a GBP
/// amount, or vice versa) would pass a currency-only assertion while still being
/// wrong, which is exactly how this shipped mislabelled before.
#[tokio::test]
async fn test_cgt_pool_reports_original_currency_for_foreign_symbol() {
    let (app, db) = test_router();
    {
        let db_lock = db.lock().unwrap();
        setup_account(&db_lock, "gia", AccountType::Investment);
        insert_event_ccy(
            &db_lock,
            "gia",
            "buy",
            "PLTR",
            "2026-05-01T10:00:00",
            "10",
            "100.00",
            None,
            "USD",
            None,
        );
    }

    let create = app
        .clone()
        .oneshot(request_json(
            Method::POST,
            "/api/currencies",
            serde_json::json!({ "code": "USD", "fx_rate": "2" }),
        ))
        .await
        .unwrap();
    assert_eq!(create.status(), StatusCode::CREATED);

    // The pool's cost basis is built from acquisitions converted at their own dates, so even a
    // pools-only request needs the acquisition-date rate stored.
    seed_rates(&app, "USD", &[("2026-05-01", "2")]).await;

    let response = app
        .oneshot(request(Method::GET, "/api/investments/pools"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let res: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let pool = res
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["symbol"] == "PLTR")
        .expect("PLTR pool");
    // Source metadata: the trades were denominated in USD.
    assert_eq!(pool["original_currency"], "USD");
    // But the value itself is base currency (GBP): 10 shares * $100 * fx_rate 2 = £2000,
    // NOT $1000 (the USD cost) and NOT $2000 (the GBP amount mislabelled as USD).
    assert_eq!(pool["total_allowable_expenditure"], "2000.00");
    assert_eq!(pool["average_cost_per_share"], "200.00");
}

/// Pins a deliberate but previously-unverified choice: when a symbol's events don't
/// all share a currency, the `/pools` endpoint refuses via
/// `check_single_currency_per_symbol` rather than letting a mixed-currency symbol
/// reach the engine — see `test_cgt_refuses_symbol_with_mixed_currencies` for the
/// `/capital-gains` equivalent. The `pool_currency` fallback in `run_cgt_engine`
/// that would pick the earliest event's currency for such a symbol is therefore
/// unreachable through the API today; this test only pins that the `/pools`
/// endpoint's own refusal fires for the same mixed-currency shape, not the
/// fallback's tie-break rule.
#[tokio::test]
async fn test_s104_pools_refuses_mixed_currency_symbol() {
    let (app, db) = test_router();
    {
        let db_lock = db.lock().unwrap();
        setup_account(&db_lock, "gia", AccountType::Investment);
        insert_event_ccy(
            &db_lock,
            "gia",
            "buy",
            "PLTR",
            "2026-05-01T10:00:00",
            "10",
            "100.00",
            None,
            "USD",
            None,
        );
        // Same symbol, different currency — the case check_single_currency_per_symbol
        // exists to catch.
        insert_event_ccy(
            &db_lock,
            "gia",
            "buy",
            "PLTR",
            "2026-05-15T10:00:00",
            "5",
            "50.00",
            None,
            "EUR",
            None,
        );
    }

    for (code, rate) in [("USD", "2"), ("EUR", "1.5")] {
        let create = app
            .clone()
            .oneshot(request_json(
                Method::POST,
                "/api/currencies",
                serde_json::json!({ "code": code, "fx_rate": rate }),
            ))
            .await
            .unwrap();
        assert_eq!(create.status(), StatusCode::CREATED);
    }

    let response = app
        .oneshot(request(Method::GET, "/api/investments/pools"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let res: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(res["code"], "mixed_symbol_currency");
    let msg = res["error"].as_str().unwrap();
    assert!(msg.contains("PLTR"), "message must name the symbol: {msg}");
}

// ── Refusal: multi-owner accounts ────────────────────────────────────────────

/// Create an investment account owned by more than one profile.
fn setup_joint_account(db: &Db, id: &str, account_type: AccountType, owners: &[&str]) {
    let account = Account {
        id: id.to_string(),
        name: format!("Joint {id}"),
        institution: "Trading 212".to_string(),
        account_type,
        currency: "GBP".to_string(),
        balance: None,
        balance_date: None,
        is_active: true,
        notes: None,
        profile_ids: owners.iter().map(|s| s.to_string()).collect(),
        is_stale: None,
        is_available: true,
    };
    db.create_account(&account).unwrap();
}

/// The S104 pool cannot split a gain between owners, so a joint investment
/// account would report 100% of the gain to each — the same gain declared on
/// two tax returns. Both CGT endpoints refuse.
#[tokio::test]
async fn test_cgt_refuses_multi_owner_investment_account() {
    let (app, db) = test_router();
    {
        let db_lock = db.lock().unwrap();
        setup_joint_account(&db_lock, "joint_gia", AccountType::Investment, &["a", "b"]);
        insert_event(
            &db_lock,
            "joint_gia",
            "buy",
            "AAPL",
            "2026-04-10T10:00:00",
            "100",
            "10.00",
            None,
        );
        insert_event(
            &db_lock,
            "joint_gia",
            "sell",
            "AAPL",
            "2026-05-10T10:00:00",
            "100",
            "15.00",
            None,
        );
    }

    let response = app
        .oneshot(request(
            Method::GET,
            "/api/investments/capital-gains?start_date=2026-04-06&end_date=2027-04-05",
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let res: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(res["code"], "multi_owner_account");
    let msg = res["error"].as_str().unwrap();
    assert!(
        msg.contains("multiple owners"),
        "message should say what is wrong: {msg}"
    );
    assert!(
        msg.contains("Joint joint_gia"),
        "message should name the account: {msg}"
    );
    assert!(
        msg.contains("split the joint account"),
        "message should say what to do next: {msg}"
    );
}

/// The pools endpoint refuses on the same grounds — the pool IS the ambiguous
/// artifact, so it must not be served either.
#[tokio::test]
async fn test_s104_pools_refuses_multi_owner_investment_account() {
    let (app, db) = test_router();
    {
        let db_lock = db.lock().unwrap();
        setup_joint_account(&db_lock, "joint_gia", AccountType::Investment, &["a", "b"]);
        insert_event(
            &db_lock,
            "joint_gia",
            "buy",
            "AAPL",
            "2026-04-10T10:00:00",
            "100",
            "10.00",
            None,
        );
    }

    let response = app
        .oneshot(request(Method::GET, "/api/investments/pools"))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let res: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(res["code"], "multi_owner_account");
}

/// Single-owner accounts are the normal case and must keep working.
#[tokio::test]
async fn test_cgt_single_owner_account_is_not_refused() {
    let (app, db) = test_router();
    {
        let db_lock = db.lock().unwrap();
        setup_account(&db_lock, "gia", AccountType::Investment);
        insert_event(
            &db_lock,
            "gia",
            "buy",
            "AAPL",
            "2026-04-10T10:00:00",
            "100",
            "10.00",
            None,
        );
        insert_event(
            &db_lock,
            "gia",
            "sell",
            "AAPL",
            "2026-05-10T10:00:00",
            "100",
            "15.00",
            None,
        );
    }

    let response = app
        .oneshot(request(
            Method::GET,
            "/api/investments/capital-gains?start_date=2026-04-06&end_date=2027-04-05",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

/// A joint account that contributes NOTHING to the computation is not grounds
/// to refuse. A joint current account is lawful, common, and irrelevant to CGT
/// — blocking on it would be an over-eager guard that breaks valid reports.
#[tokio::test]
async fn test_cgt_ignores_multi_owner_account_with_no_investment_events() {
    let (app, db) = test_router();
    {
        let db_lock = db.lock().unwrap();
        setup_account(&db_lock, "gia", AccountType::Investment);
        // Joint current account, no investment events against it.
        setup_joint_account(
            &db_lock,
            "joint_current",
            AccountType::Checking,
            &["a", "b"],
        );
        insert_event(
            &db_lock,
            "gia",
            "buy",
            "AAPL",
            "2026-04-10T10:00:00",
            "100",
            "10.00",
            None,
        );
        insert_event(
            &db_lock,
            "gia",
            "sell",
            "AAPL",
            "2026-05-10T10:00:00",
            "100",
            "15.00",
            None,
        );
    }

    let response = app
        .oneshot(request(
            Method::GET,
            "/api/investments/capital-gains?start_date=2026-04-06&end_date=2027-04-05",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

/// A joint ISA is excluded from CGT anyway (gains are tax-free), so it must not
/// block a report it contributes nothing to.
#[tokio::test]
async fn test_cgt_ignores_multi_owner_isa_account() {
    let (app, db) = test_router();
    {
        let db_lock = db.lock().unwrap();
        setup_account(&db_lock, "gia", AccountType::Investment);
        setup_joint_account(
            &db_lock,
            "joint_isa",
            AccountType::InvestmentIsa,
            &["a", "b"],
        );
        // Events in the joint ISA — excluded by account_type before the owner check.
        insert_event(
            &db_lock,
            "joint_isa",
            "buy",
            "VUSA",
            "2026-04-10T10:00:00",
            "50",
            "80.00",
            None,
        );
        insert_event(
            &db_lock,
            "gia",
            "buy",
            "AAPL",
            "2026-04-10T10:00:00",
            "100",
            "10.00",
            None,
        );
        insert_event(
            &db_lock,
            "gia",
            "sell",
            "AAPL",
            "2026-05-10T10:00:00",
            "100",
            "15.00",
            None,
        );
    }

    let response = app
        .oneshot(request(
            Method::GET,
            "/api/investments/capital-gains?start_date=2026-04-06&end_date=2027-04-05",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

// ── Refusal: one symbol, two currencies ──────────────────────────────────────

/// Report-time precheck. Uses two ordinary ISO currencies (GBP/USD) rather than
/// a sub-unit pair: since sub-unit conversion (plan 23 §0.2 (7.1)) now happens
/// inside `create_investment_event` itself — the same write path this helper
/// calls — a GBX-labelled insert here would be converted to GBP before it ever
/// reached storage, and the two rows would no longer be mixed-currency at all.
/// Events are still inserted straight through the Db rather than through the
/// API, so this also covers rows written before the write-time guard existed.
#[tokio::test]
async fn test_cgt_refuses_symbol_with_mixed_currencies() {
    let (app, db) = test_router();
    {
        let db_lock = db.lock().unwrap();
        setup_account(&db_lock, "gia", AccountType::Investment);
        // Same ticker collision, two currencies — the realistic cause is a
        // dual-listed security reported in each market's own currency.
        insert_event_ccy(
            &db_lock,
            "gia",
            "buy",
            "VOD",
            "2026-04-10T10:00:00",
            "100",
            "50.00",
            None,
            "USD",
            None,
        );
        insert_event_ccy(
            &db_lock,
            "gia",
            "buy",
            "VOD",
            "2026-04-20T10:00:00",
            "100",
            "75.00",
            None,
            "GBP",
            None,
        );
    }

    // Configure USD so this fails on the mixed-currency rule, not missing_currencies.
    let create = app
        .clone()
        .oneshot(request_json(
            Method::POST,
            "/api/currencies",
            serde_json::json!({ "code": "USD", "fx_rate": "0.8" }),
        ))
        .await
        .unwrap();
    assert_eq!(create.status(), StatusCode::CREATED);

    let response = app
        .oneshot(request(
            Method::GET,
            "/api/investments/capital-gains?start_date=2026-04-06&end_date=2027-04-05",
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let res: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(res["code"], "mixed_symbol_currency");
    let msg = res["error"].as_str().unwrap();
    assert!(msg.contains("VOD"), "message must name the symbol: {msg}");
    assert!(
        msg.contains("GBP") && msg.contains("USD"),
        "message must name both currencies: {msg}"
    );
    assert!(
        msg.contains("so each symbol uses one currency"),
        "message must say what to do next: {msg}"
    );
}

/// A fee in a different currency from the trade is legitimate and already
/// handled by the engine — it must NOT trip the mixed-currency guard, which
/// looks only at the trade currency.
#[tokio::test]
async fn test_cgt_allows_fee_currency_differing_from_trade_currency() {
    let (app, db) = test_router();
    {
        let db_lock = db.lock().unwrap();
        setup_account(&db_lock, "gia", AccountType::Investment);
        insert_event_ccy(
            &db_lock,
            "gia",
            "buy",
            "AAPL",
            "2026-04-10T10:00:00",
            "100",
            "10.00",
            Some("30.00"),
            "USD",
            Some("GBP"),
        );
        insert_event_ccy(
            &db_lock,
            "gia",
            "sell",
            "AAPL",
            "2026-05-10T10:00:00",
            "100",
            "15.00",
            Some("5.00"),
            "USD",
            Some("GBP"),
        );
    }

    let create = app
        .clone()
        .oneshot(request_json(
            Method::POST,
            "/api/currencies",
            serde_json::json!({ "code": "USD", "fx_rate": "2" }),
        ))
        .await
        .unwrap();
    assert_eq!(create.status(), StatusCode::CREATED);

    // A date-keyed rate per leg; the engine refuses rather than using the flat rate.
    seed_rates(&app, "USD", &[("2026-04-10", "2"), ("2026-05-10", "2")]).await;

    let response = app
        .oneshot(request(
            Method::GET,
            "/api/investments/capital-gains?start_date=2026-04-06&end_date=2027-04-05",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

/// Two DIFFERENT symbols in two different currencies is normal for any
/// international portfolio and must not be refused.
#[tokio::test]
async fn test_cgt_allows_different_symbols_in_different_currencies() {
    let (app, db) = test_router();
    {
        let db_lock = db.lock().unwrap();
        setup_account(&db_lock, "gia", AccountType::Investment);
        insert_event_ccy(
            &db_lock,
            "gia",
            "buy",
            "AAPL",
            "2026-04-10T10:00:00",
            "100",
            "10.00",
            None,
            "USD",
            None,
        );
        insert_event_ccy(
            &db_lock,
            "gia",
            "sell",
            "AAPL",
            "2026-05-10T10:00:00",
            "100",
            "15.00",
            None,
            "USD",
            None,
        );
        insert_event_ccy(
            &db_lock,
            "gia",
            "buy",
            "VOD",
            "2026-04-10T10:00:00",
            "100",
            "75.00",
            None,
            "GBP",
            None,
        );
    }

    let create = app
        .clone()
        .oneshot(request_json(
            Method::POST,
            "/api/currencies",
            serde_json::json!({ "code": "USD", "fx_rate": "2" }),
        ))
        .await
        .unwrap();
    assert_eq!(create.status(), StatusCode::CREATED);

    // Only the USD symbol needs rates; the GBP one converts to itself at 1.
    seed_rates(&app, "USD", &[("2026-04-10", "2"), ("2026-05-10", "2")]).await;

    let response = app
        .oneshot(request(
            Method::GET,
            "/api/investments/capital-gains?start_date=2026-04-06&end_date=2027-04-05",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

// ── Refusal: write-time symbol/currency guard ────────────────────────────────

/// POST rejects an event whose symbol already exists under another currency.
///
/// Uses USD for the seed event rather than a sub-unit code: sub-unit conversion
/// (plan 23 §0.2 (7.1)) now happens inside `create_investment_event`, the same
/// write path `insert_event_ccy` calls, so a GBX seed would already be stored
/// as GBP and could never conflict with a second GBP event.
#[tokio::test]
async fn test_create_investment_rejects_conflicting_symbol_currency() {
    let (app, db) = test_router();
    {
        let db_lock = db.lock().unwrap();
        setup_account(&db_lock, "gia", AccountType::Investment);
        insert_event_ccy(
            &db_lock,
            "gia",
            "buy",
            "VOD",
            "2026-04-10T10:00:00",
            "100",
            "50.00",
            None,
            "USD",
            None,
        );
    }

    let create = app
        .clone()
        .oneshot(request_json(
            Method::POST,
            "/api/currencies",
            serde_json::json!({ "code": "USD", "fx_rate": "0.8" }),
        ))
        .await
        .unwrap();
    assert_eq!(create.status(), StatusCode::CREATED);

    let response = app
        .oneshot(request_json(
            Method::POST,
            "/api/investments",
            serde_json::json!({
                "account_id": "gia",
                "event_type": "buy",
                "symbol": "VOD",
                "date": "2026-04-20T10:00:00",
                "quantity": "100",
                "price_per_share": "75.00",
                "currency": "GBP",
            }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let res: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(res["code"], "symbol_currency_conflict");
    let msg = res["error"].as_str().unwrap();
    assert!(msg.contains("VOD"), "message must name the symbol: {msg}");
    assert!(
        msg.contains("Correct this event"),
        "message must say what to do next: {msg}"
    );
}

/// The same symbol in the SAME currency is the normal case — adding a second
/// buy must still work.
#[tokio::test]
async fn test_create_investment_allows_matching_symbol_currency() {
    let (app, db) = test_router();
    {
        let db_lock = db.lock().unwrap();
        setup_account(&db_lock, "gia", AccountType::Investment);
        insert_event(
            &db_lock,
            "gia",
            "buy",
            "AAPL",
            "2026-04-10T10:00:00",
            "100",
            "10.00",
            None,
        );
    }

    let response = app
        .oneshot(request_json(
            Method::POST,
            "/api/investments",
            serde_json::json!({
                "account_id": "gia",
                "event_type": "buy",
                "symbol": "AAPL",
                "date": "2026-04-20T10:00:00",
                "quantity": "50",
                "price_per_share": "12.00",
                "currency": "GBP",
            }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

/// A brand-new symbol in any currency is unconstrained.
#[tokio::test]
async fn test_create_investment_allows_new_symbol_in_other_currency() {
    let (app, db) = test_router();
    {
        let db_lock = db.lock().unwrap();
        setup_account(&db_lock, "gia", AccountType::Investment);
        insert_event(
            &db_lock,
            "gia",
            "buy",
            "AAPL",
            "2026-04-10T10:00:00",
            "100",
            "10.00",
            None,
        );
    }

    let create = app
        .clone()
        .oneshot(request_json(
            Method::POST,
            "/api/currencies",
            serde_json::json!({ "code": "USD", "fx_rate": "2" }),
        ))
        .await
        .unwrap();
    assert_eq!(create.status(), StatusCode::CREATED);

    let response = app
        .oneshot(request_json(
            Method::POST,
            "/api/investments",
            serde_json::json!({
                "account_id": "gia",
                "event_type": "buy",
                "symbol": "MSFT",
                "date": "2026-04-20T10:00:00",
                "quantity": "10",
                "price_per_share": "300.00",
                "currency": "USD",
            }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

/// PATCHing an event to a currency that conflicts with its symbol is rejected.
///
/// Uses USD rather than a sub-unit code: `update_investment_event` (the PATCH
/// write path) writes `currency` raw and is not in the list of write paths
/// plan 23 §0.2 (7.1) made sub-unit-aware (that's `create_investment_event`,
/// `HoldingWrite::into_holding`, `Transaction::from_unified`,
/// `insert_transactions_bulk`) — PATCHing to a sub-unit code was never
/// intended to be supported, and now correctly fails `validate_currency`
/// before reaching the conflict check this test targets.
#[tokio::test]
async fn test_patch_investment_rejects_conflicting_symbol_currency() {
    let (app, db) = test_router();
    let event_id = {
        let db_lock = db.lock().unwrap();
        setup_account(&db_lock, "gia", AccountType::Investment);
        insert_event(
            &db_lock,
            "gia",
            "buy",
            "VOD",
            "2026-04-10T10:00:00",
            "100",
            "75.00",
            None,
        );
        insert_event(
            &db_lock,
            "gia",
            "buy",
            "VOD",
            "2026-04-20T10:00:00",
            "100",
            "76.00",
            None,
        );
        let events = db_lock
            .list_investment_events(None, Some("VOD"), None, None)
            .unwrap();
        events[0].id.clone()
    };

    let create = app
        .clone()
        .oneshot(request_json(
            Method::POST,
            "/api/currencies",
            serde_json::json!({ "code": "USD", "fx_rate": "0.8" }),
        ))
        .await
        .unwrap();
    assert_eq!(create.status(), StatusCode::CREATED);

    // Moving ONE of the two VOD events to USD would split the symbol.
    let response = app
        .oneshot(request_json(
            Method::PATCH,
            &format!("/api/investments/{event_id}"),
            serde_json::json!({ "currency": "USD" }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let res: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(res["code"], "symbol_currency_conflict");
}

/// Re-denominating the ONLY event of a symbol is legitimate — the guard must
/// exclude the row being edited, or it would conflict with itself.
///
/// Uses USD rather than a sub-unit code — see
/// `test_patch_investment_rejects_conflicting_symbol_currency` above for why.
#[tokio::test]
async fn test_patch_investment_allows_recurrency_of_sole_event() {
    let (app, db) = test_router();
    let event_id = {
        let db_lock = db.lock().unwrap();
        setup_account(&db_lock, "gia", AccountType::Investment);
        insert_event(
            &db_lock,
            "gia",
            "buy",
            "VOD",
            "2026-04-10T10:00:00",
            "100",
            "75.00",
            None,
        );
        let events = db_lock
            .list_investment_events(None, Some("VOD"), None, None)
            .unwrap();
        events[0].id.clone()
    };

    let create = app
        .clone()
        .oneshot(request_json(
            Method::POST,
            "/api/currencies",
            serde_json::json!({ "code": "USD", "fx_rate": "0.8" }),
        ))
        .await
        .unwrap();
    assert_eq!(create.status(), StatusCode::CREATED);

    let response = app
        .oneshot(request_json(
            Method::PATCH,
            &format!("/api/investments/{event_id}"),
            serde_json::json!({ "currency": "USD" }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

/// An import batch that carries one symbol in two currencies is refused whole —
/// nothing is written, so the user can correct and re-import.
///
/// No longer registers GBX via POST /api/currencies: the within-batch conflict
/// check (`validate_symbol_currency` in `import_investments`) compares each
/// event's raw, pre-conversion currency string, so GBX vs GBP still conflicts
/// correctly without GBX itself being a "configured" currency — and per plan
/// 23 §0.2 (7.1), GBX can no longer be registered as one at all (only its
/// parent GBP, already the default, needs to be configured).
#[tokio::test]
async fn test_import_investments_rejects_mixed_currency_within_batch() {
    let (app, db) = test_router();
    {
        let db_lock = db.lock().unwrap();
        setup_account(&db_lock, "gia", AccountType::Investment);
    }

    let response = app
        .clone()
        .oneshot(request_json(
            Method::POST,
            "/api/investments/import",
            serde_json::json!({
                "account_id": "gia",
                "events": [
                    {
                        "account_id": "gia",
                        "event_type": "buy",
                        "symbol": "VOD",
                        "date": "2026-04-10T10:00:00",
                        "quantity": "100",
                        "price_per_share": "7500",
                        "fee": null,
                        "currency": "GBX",
                        "notes": null,
                        "source_document_ids": [],
                    },
                    {
                        "account_id": "gia",
                        "event_type": "buy",
                        "symbol": "VOD",
                        "date": "2026-04-20T10:00:00",
                        "quantity": "100",
                        "price_per_share": "75.00",
                        "fee": null,
                        "currency": "GBP",
                        "notes": null,
                        "source_document_ids": [],
                    },
                ],
            }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let res: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(res["code"], "symbol_currency_conflict");

    // Nothing was written — the batch is refused before any insert.
    let db_lock = db.lock().unwrap();
    let events = db_lock
        .list_investment_events(None, None, None, None)
        .unwrap();
    assert!(
        events.is_empty(),
        "a refused batch must write nothing, found {} events",
        events.len()
    );
}

/// A consistent import batch still imports.
#[tokio::test]
async fn test_import_investments_allows_consistent_currencies() {
    let (app, db) = test_router();
    {
        let db_lock = db.lock().unwrap();
        setup_account(&db_lock, "gia", AccountType::Investment);
    }

    let create = app
        .clone()
        .oneshot(request_json(
            Method::POST,
            "/api/currencies",
            serde_json::json!({ "code": "USD", "fx_rate": "2" }),
        ))
        .await
        .unwrap();
    assert_eq!(create.status(), StatusCode::CREATED);

    let response = app
        .oneshot(request_json(
            Method::POST,
            "/api/investments/import",
            serde_json::json!({
                "account_id": "gia",
                "events": [
                    {
                        "account_id": "gia",
                        "event_type": "buy",
                        "symbol": "VOD",
                        "date": "2026-04-10T10:00:00",
                        "quantity": "100",
                        "price_per_share": "75.00",
                        "fee": null,
                        "currency": "GBP",
                        "notes": null,
                        "source_document_ids": [],
                    },
                    {
                        "account_id": "gia",
                        "event_type": "buy",
                        "symbol": "AAPL",
                        "date": "2026-04-20T10:00:00",
                        "quantity": "10",
                        "price_per_share": "200.00",
                        "fee": null,
                        "currency": "USD",
                        "notes": null,
                        "source_document_ids": [],
                    },
                ],
            }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let res: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(res["inserted"], 2);
}

/// `disposal_groups`: two disposals of the SAME symbol on DIFFERENT dates must stay
/// two groups, never merge into one just because the symbol matches — the grouping
/// key is (symbol, disposal_date), not symbol alone.
#[tokio::test]
async fn test_cgt_disposal_groups_same_symbol_different_dates_stay_separate() {
    let (app, db) = test_router();
    {
        let db_lock = db.lock().unwrap();
        setup_account(&db_lock, "gia", AccountType::Investment);

        // Pool: 200 NVDA @ 10 (cost 2000, avg 10.00)
        insert_event(
            &db_lock,
            "gia",
            "buy",
            "NVDA",
            "2026-05-01T10:00:00",
            "200",
            "10.00",
            None,
        );
        // Sell 50 on May 10th — pure S104, gain = 50*(30-10) = 1000
        insert_event(
            &db_lock,
            "gia",
            "sell",
            "NVDA",
            "2026-05-10T10:00:00",
            "50",
            "30.00",
            None,
        );
        // Sell 50 more on May 20th — same symbol, different date, gain = 50*(40-10) = 1500
        insert_event(
            &db_lock,
            "gia",
            "sell",
            "NVDA",
            "2026-05-20T10:00:00",
            "50",
            "40.00",
            None,
        );
    }

    let response = app
        .oneshot(request(
            Method::GET,
            "/api/investments/capital-gains?start_date=2026-04-06&end_date=2027-04-05",
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let res: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // Two realized_events (each pure S104, no multi-rule splitting here) must
    // remain two disposal_groups, NOT collapse to one just because they share a symbol.
    assert_eq!(res["realized_events"].as_array().unwrap().len(), 2);
    let groups = res["disposal_groups"].as_array().unwrap();
    assert_eq!(groups.len(), 2);

    let g_may10 = groups
        .iter()
        .find(|g| g["disposal_date"] == "2026-05-10 10:00:00")
        .unwrap();
    assert_eq!(g_may10["symbol"], "NVDA");
    assert_eq!(g_may10["quantity"], "50");
    assert_eq!(g_may10["proceeds"], "1500.00");
    assert_eq!(g_may10["cost_basis"], "500.00");
    assert_eq!(g_may10["gain_loss"], "1000.00");

    let g_may20 = groups
        .iter()
        .find(|g| g["disposal_date"] == "2026-05-20 10:00:00")
        .unwrap();
    assert_eq!(g_may20["symbol"], "NVDA");
    assert_eq!(g_may20["quantity"], "50");
    assert_eq!(g_may20["proceeds"], "2000.00");
    assert_eq!(g_may20["cost_basis"], "500.00");
    assert_eq!(g_may20["gain_loss"], "1500.00");
}

/// Contract test for decision 7.3's "absent `start_date` means from time zero":
/// a query with only `end_date` set must include disposals from BEFORE the tax
/// year the report's date range would otherwise suggest, proving there is no
/// implicit lower bound at all (not "start of this tax year", not "start of
/// this calendar year" — genuinely unbounded).
#[tokio::test]
async fn test_cgt_absent_start_date_is_from_time_zero() {
    let (app, db) = test_router();
    {
        let db_lock = db.lock().unwrap();
        setup_account(&db_lock, "gia", AccountType::Investment);

        // A disposal years before any plausible "current" tax year.
        insert_event(
            &db_lock,
            "gia",
            "buy",
            "IBM",
            "2019-01-01T10:00:00",
            "10",
            "100.00",
            None,
        );
        insert_event(
            &db_lock,
            "gia",
            "sell",
            "IBM",
            "2019-06-01T10:00:00",
            "10",
            "150.00",
            None,
        );
    }

    // Only `end_date` set, far in the future — no `start_date` at all.
    let response = app
        .oneshot(request(
            Method::GET,
            "/api/investments/capital-gains?end_date=2026-01-01",
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let res: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let realized = res["realized_events"].as_array().unwrap();
    assert_eq!(realized.len(), 1);
    assert_eq!(realized[0]["symbol"], "IBM");
    assert_eq!(realized[0]["quantity"], "10");
    assert_eq!(realized[0]["proceeds"], "1500.00");
    assert_eq!(realized[0]["cost_basis"], "1000.00");
    assert_eq!(realized[0]["gain_loss"], "500.00");
}

// ── Historical (date-keyed) FX ───────────────────────────────────────────────

/// The whole point of the feature: each leg converts at ITS OWN date's rate.
///
/// Acquisition and disposal are given deliberately different rates, so the expected numbers are
/// only reachable if the engine looks up per date. Under the old flat-rate behaviour — one rate
/// applied to every event regardless of date — both legs would use the same number and the
/// gain would be wrong; that is precisely the ~6% error against the owner's filed return that
/// this feature exists to remove.
#[tokio::test]
async fn test_cgt_uses_date_specific_rates() {
    let (app, db) = test_router();
    {
        let db_lock = db.lock().unwrap();
        setup_account(&db_lock, "gia", AccountType::Investment);

        // Buy 100 @ $10 on 2026-05-01, sell 100 @ $12 on 2026-06-01.
        insert_event_ccy(
            &db_lock,
            "gia",
            "buy",
            "AAPL",
            "2026-05-01T10:00:00",
            "100",
            "10.00",
            None,
            "USD",
            None,
        );
        insert_event_ccy(
            &db_lock,
            "gia",
            "sell",
            "AAPL",
            "2026-06-01T10:00:00",
            "100",
            "12.00",
            None,
            "USD",
            None,
        );
    }

    let create = app
        .clone()
        .oneshot(request_json(
            Method::POST,
            "/api/currencies",
            // The flat rate is deliberately neither of the historical rates: if the engine ever
            // falls back to it, the numbers below cannot come out right.
            serde_json::json!({ "code": "USD", "fx_rate": "0.5" }),
        ))
        .await
        .unwrap();
    assert_eq!(create.status(), StatusCode::CREATED);

    // Acquisition at 0.80, disposal at 0.60 — a falling rate, so a position that gained in USD
    // makes a LOSS in GBP. No single flat rate can produce this pair of numbers.
    seed_rates(
        &app,
        "USD",
        &[("2026-05-01", "0.80"), ("2026-06-01", "0.60")],
    )
    .await;

    let response = app
        .oneshot(request(
            Method::GET,
            "/api/investments/capital-gains?start_date=2026-04-06&end_date=2027-04-05",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let res: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let realized = res["realized_events"].as_array().unwrap();
    assert_eq!(realized.len(), 1);
    let event = &realized[0];

    // Cost basis: 100 * $10 = $1000 at the 1 May rate 0.80 -> £800.00
    assert_eq!(event["cost_basis"], "800.0000");
    // Proceeds:   100 * $12 = $1200 at the 1 Jun rate 0.60 -> £720.00
    assert_eq!(event["proceeds"], "720.0000");
    // A $200 profit becomes an £80 LOSS once each leg is converted at its own date.
    assert_eq!(event["gain_loss"], "-80.0000");
}

/// A missing rate must list EVERY missing pair in one response, not just the first.
///
/// One round-trip has to tell the user everything to supply; discovering ~49 missing rates one
/// request at a time would be unusable. Also asserts the distinct `missing_exchange_rates` code
/// (not `missing_currencies`, which means the currency has no row at all).
#[tokio::test]
async fn test_cgt_missing_exchange_rates_lists_every_pair() {
    let (app, db) = test_router();
    {
        let db_lock = db.lock().unwrap();
        setup_account(&db_lock, "gia", AccountType::Investment);
        insert_event_ccy(
            &db_lock,
            "gia",
            "buy",
            "AAPL",
            "2026-05-01T10:00:00",
            "100",
            "10.00",
            None,
            "USD",
            None,
        );
        insert_event_ccy(
            &db_lock,
            "gia",
            "sell",
            "AAPL",
            "2026-06-01T10:00:00",
            "40",
            "12.00",
            None,
            "USD",
            None,
        );
    }

    let create = app
        .clone()
        .oneshot(request_json(
            Method::POST,
            "/api/currencies",
            serde_json::json!({ "code": "USD", "fx_rate": "0.75" }),
        ))
        .await
        .unwrap();
    assert_eq!(create.status(), StatusCode::CREATED);

    // Seed only ONE of the two required dates, so the response must report exactly the other.
    seed_rates(&app, "USD", &[("2026-05-01", "0.80")]).await;

    let response = app
        .oneshot(request(
            Method::GET,
            "/api/investments/capital-gains?start_date=2026-04-06&end_date=2027-04-05",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let res: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(res["code"], "missing_exchange_rates");
    assert_eq!(res["quote"], "GBP");
    let missing = res["missing"].as_array().unwrap();
    assert_eq!(missing.len(), 1, "only the unseeded date should be missing");
    assert_eq!(missing[0]["currency"], "USD");
    assert_eq!(missing[0]["date"], "2026-06-01");
}

/// A report for one tax year needs rates for acquisitions from EARLIER years.
///
/// The S104 pool is built from every acquisition ever, so its cost basis depends on rates going
/// back as far as the ledger. Collecting only dates inside the requested window would leave the
/// pool built at the wrong rates and silently produce a wrong cost basis — the report would look
/// complete and be wrong. This is the subtlety that makes the precheck walk the event set rather
/// than the date filter.
#[tokio::test]
async fn test_cgt_precheck_requires_rates_for_prior_year_acquisitions() {
    let (app, db) = test_router();
    {
        let db_lock = db.lock().unwrap();
        setup_account(&db_lock, "gia", AccountType::Investment);
        // Acquired two tax years before the reporting window.
        insert_event_ccy(
            &db_lock,
            "gia",
            "buy",
            "AAPL",
            "2023-06-15T10:00:00",
            "100",
            "10.00",
            None,
            "USD",
            None,
        );
        // Disposed inside 2026-27.
        insert_event_ccy(
            &db_lock,
            "gia",
            "sell",
            "AAPL",
            "2026-06-01T10:00:00",
            "50",
            "20.00",
            None,
            "USD",
            None,
        );
    }

    let create = app
        .clone()
        .oneshot(request_json(
            Method::POST,
            "/api/currencies",
            serde_json::json!({ "code": "USD", "fx_rate": "0.75" }),
        ))
        .await
        .unwrap();
    assert_eq!(create.status(), StatusCode::CREATED);

    // Seed only the in-window disposal date. The 2023 acquisition is outside the requested tax
    // year, so a precheck that walked the date filter would think it had everything it needed.
    seed_rates(&app, "USD", &[("2026-06-01", "0.60")]).await;

    let response = app
        .clone()
        .oneshot(request(
            Method::GET,
            "/api/investments/capital-gains?start_date=2026-04-06&end_date=2027-04-05",
        ))
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "the out-of-window acquisition rate must still be required"
    );
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let res: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(res["code"], "missing_exchange_rates");
    let missing = res["missing"].as_array().unwrap();
    assert_eq!(missing.len(), 1);
    assert_eq!(
        missing[0]["date"], "2023-06-15",
        "the prior-year acquisition date is what is missing"
    );

    // Supplying it lets the report generate, and the cost basis uses the 2023 rate.
    seed_rates(&app, "USD", &[("2023-06-15", "0.80")]).await;
    let response = app
        .oneshot(request(
            Method::GET,
            "/api/investments/capital-gains?start_date=2026-04-06&end_date=2027-04-05",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let res: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let realized = res["realized_events"].as_array().unwrap();
    assert_eq!(realized.len(), 1);
    // 50 shares at $10 = $500 at the 2023 rate 0.80 -> £400.
    assert_eq!(realized[0]["cost_basis"], "400.0000");
    // 50 shares at $20 = $1000 at the 2026 rate 0.60 -> £600.
    assert_eq!(realized[0]["proceeds"], "600.0000");
}

/// A GBP-only portfolio needs no rates at all: the preferred currency converts to itself at 1
/// and must never be prompted for. Guards against the precheck demanding GBP->GBP rates, which
/// would make every existing GBP report unusable.
#[tokio::test]
async fn test_cgt_preferred_currency_needs_no_rates() {
    let (app, db) = test_router();
    {
        let db_lock = db.lock().unwrap();
        setup_account(&db_lock, "gia", AccountType::Investment);
        insert_event(
            &db_lock,
            "gia",
            "buy",
            "TSCO",
            "2026-05-01T10:00:00",
            "100",
            "10.00",
            None,
        );
        insert_event(
            &db_lock,
            "gia",
            "sell",
            "TSCO",
            "2026-06-01T10:00:00",
            "100",
            "12.00",
            None,
        );
    }

    let response = app
        .oneshot(request(
            Method::GET,
            "/api/investments/capital-gains?start_date=2026-04-06&end_date=2027-04-05",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let res: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let realized = res["realized_events"].as_array().unwrap();
    assert_eq!(realized.len(), 1);
    assert_eq!(realized[0]["gain_loss"], "200.00");
}
