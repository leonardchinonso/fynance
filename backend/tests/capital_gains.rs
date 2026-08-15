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

    // `disposal_groups`: 4 realized_events rows above collapse to 3 groups, because
    // the July 1st sale of 100 shares matched TWO rules (30-Day Rule for 50 + S104
    // Pool for the remaining 50) and is therefore ONE actual sale, not two. A test
    // that only checked the row count (4 -> 3) would still pass if the summed
    // figures were wrong, so assert the summed quantity/proceeds/cost/gain, not
    // just the count.
    let groups = res["disposal_groups"].as_array().unwrap();
    assert_eq!(groups.len(), 3);

    // June 1st: Same-Day only — single-rule disposal, group mirrors m1 exactly.
    let g_jun1 = groups
        .iter()
        .find(|g| g["disposal_date"] == "2026-06-01 10:00:00")
        .unwrap();
    assert_eq!(g_jun1["quantity"], "50");
    assert_eq!(g_jun1["proceeds"], "900.00");
    assert_eq!(g_jun1["cost_basis"], "800.00");
    assert_eq!(g_jun1["gain_loss"], "100.00");
    assert_eq!(g_jun1["events"].as_array().unwrap().len(), 1);

    // July 1st: the multi-rule disposal. 30-Day (qty 50, proceeds 1000, cost 750)
    // + S104 (qty 50, proceeds 1000, cost 600) must SUM to qty 100 / proceeds 2000 /
    // cost 1350 / gain 650 — not just "2 events became 1".
    let g_jul1 = groups
        .iter()
        .find(|g| g["disposal_date"] == "2026-07-01 09:00:00")
        .unwrap();
    assert_eq!(g_jul1["quantity"], "100");
    assert_eq!(g_jul1["proceeds"], "2000.00");
    assert_eq!(g_jul1["cost_basis"], "1350.00");
    assert_eq!(g_jul1["gain_loss"], "650.00");
    assert_eq!(g_jul1["events"].as_array().unwrap().len(), 2);

    // August 1st: S104 only, but a *different date* from the July 1st S104 match —
    // must stay its own group, not merge into g_jul1 just because rule_applied matches.
    let g_aug1 = groups
        .iter()
        .find(|g| g["disposal_date"] == "2026-08-01 09:00:00")
        .unwrap();
    assert_eq!(g_aug1["quantity"], "100");
    assert_eq!(g_aug1["proceeds"], "2500.00");
    assert_eq!(g_aug1["cost_basis"], "1200.00");
    assert_eq!(g_aug1["gain_loss"], "1300.00");
    assert_eq!(g_aug1["events"].as_array().unwrap().len(), 1);

    // Grouped totals must reconcile with the ungrouped summary — rolling up by
    // (symbol, disposal_date) must not silently drop or double-count money.
    let group_proceeds: rust_decimal::Decimal = groups
        .iter()
        .map(|g| {
            g["proceeds"]
                .as_str()
                .unwrap()
                .parse::<rust_decimal::Decimal>()
                .unwrap()
        })
        .sum();
    let group_gain: rust_decimal::Decimal = groups
        .iter()
        .map(|g| {
            g["gain_loss"]
                .as_str()
                .unwrap()
                .parse::<rust_decimal::Decimal>()
                .unwrap()
        })
        .sum();
    assert_eq!(group_proceeds.to_string(), "5400.00");
    assert_eq!(group_gain.to_string(), "2050.00");
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
async fn test_cgt_short_sales_unmatched() {
    let (app, db) = test_router();
    {
        let db_lock = db.lock().unwrap();
        setup_account(&db_lock, "gia", AccountType::Investment);

        // Sell 100 AAPL @ 15.00 with no acquisitions -> Short Sale / Unmatched remainder
        // Proceeds = 1500, Cost basis = 0, Gain = 1500
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

    let event = &realized[0];
    assert_eq!(event["quantity"], "100");
    assert_eq!(event["proceeds"], "1500.00");
    assert_eq!(event["cost_basis"], "0");
    assert_eq!(event["gain_loss"], "1500.00");
    assert_eq!(event["rule_applied"], "Unmatched");
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
    // Pool figures are in the symbol's native currency, so the pool must say which.
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

/// A pool holding a non-GBP symbol reports that symbol's own currency, not the
/// preferred one. Without this the frontend falls back to the base currency and
/// mislabels the pool workings for any symbol with no disposals in the window.
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
    assert_eq!(pool["original_currency"], "USD");
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
