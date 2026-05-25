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

fn setup_gia_account(db: &Db, id: &str) {
    let account = Account {
        id: id.to_string(),
        name: "Taxable GIA Brokerage".to_string(),
        institution: "Trading 212".to_string(),
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
        notes: None,
    };
    db.create_investment_event(&body).unwrap();
}

#[tokio::test]
async fn test_cgt_same_day_matching() {
    let (app, db) = test_router();
    {
        let db_lock = db.lock().unwrap();
        setup_gia_account(&db_lock, "gia");

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
            "/api/investments/capital-gains?tax_year=2026-27",
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
        setup_gia_account(&db_lock, "gia");

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
            "/api/investments/capital-gains?tax_year=2026-27",
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
        setup_gia_account(&db_lock, "gia");

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
            "/api/investments/capital-gains?tax_year=2026-27",
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
        setup_gia_account(&db_lock, "gia");

        // Buy 100 MSFT @ 10 (total cost 1000). Split 2:1 on May 5th. Sell 100 MSFT @ 8.
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
            "2",
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
            "/api/investments/capital-gains?tax_year=2026-27",
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
        setup_gia_account(&db_lock, "gia");

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

        // 2. Stock Split of 10:1 on May 15th
        insert_event(
            &db_lock,
            "gia",
            "split",
            "GOOG",
            "2026-05-15T09:00:00",
            "10",
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
            "/api/investments/capital-gains?tax_year=2026-27",
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
