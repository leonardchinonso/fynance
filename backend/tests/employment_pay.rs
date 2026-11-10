//! Employment-pay (PAYE) CRUD and the `GET /api/tax/summary/:tax_year`
//! endpoint (plan 28). CRUD goes through the router so status codes and
//! error codes are pinned too; seed data goes straight to storage.
//! All data synthetic.

use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode};
use chrono::NaiveDateTime;
use fynance::model::{
    Account, AccountType, CategoryType, CreateCategoryPayload, EmploymentPayPayload,
    ImportTransaction,
};
use fynance::server::build_router;
use fynance::storage::{Db, TransactionFilters};
use rust_decimal::Decimal;
use serde_json::{Value, json};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;
use tower::ServiceExt;

macro_rules! dec {
    ($val:expr) => {
        Decimal::from_str(stringify!($val)).unwrap()
    };
}

fn test_router() -> (axum::Router, Arc<Mutex<Db>>) {
    let dir = tempdir().unwrap();
    let path = dir.keep().join("employment_pay_test.db");
    let db = Db::open(&path).unwrap();
    let shared = Arc::new(Mutex::new(db));
    (build_router(shared.clone(), true), shared)
}

/// One HTTP call, returning the status plus the body parsed as JSON when it
/// is JSON (an axum payload-rejection body is plain text, kept as a String).
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

/// The minimal valid POST body; `emp` layers one-field mutations on top.
fn emp_base() -> Value {
    json!({
        "tax_year": "2025-26",
        "employer": "Employer A",
        "gross_pay": "1000.00",
        "tax_deducted": "200.00",
    })
}

fn emp(overrides: Value) -> Value {
    let mut base = emp_base().as_object().unwrap().clone();
    base.extend(overrides.as_object().unwrap().clone());
    Value::Object(base)
}

async fn post_emp(app: &axum::Router, body: Value) -> (StatusCode, Value) {
    call(app, Method::POST, "/api/employment-pay", Some(body)).await
}

/// Insert an employment row straight through storage from a JSON payload.
fn seed_emp(db: &Db, body: Value) -> String {
    let payload: EmploymentPayPayload = serde_json::from_value(body).unwrap();
    db.create_employment_pay(&payload).unwrap().id
}

fn make_account(db: &Db, id: &str, name: &str) {
    db.create_account(&Account {
        id: id.to_string(),
        name: name.to_string(),
        institution: "TestBank".to_string(),
        account_type: AccountType::Savings,
        currency: "GBP".to_string(),
        balance: None,
        balance_date: None,
        is_active: true,
        notes: None,
        profile_ids: vec!["default".to_string()],
        is_stale: None,
        is_available: true,
    })
    .unwrap();
}

fn leaf(
    db: &Db,
    parent_name: &str,
    name: &str,
    category_type: CategoryType,
) -> fynance::model::Category {
    let parent = db
        .create_category(&CreateCategoryPayload {
            name: parent_name.to_string(),
            parent_id: None,
            display_order: None,
            description: None,
            category_type,
        })
        .unwrap();
    db.create_category(&CreateCategoryPayload {
        name: name.to_string(),
        parent_id: Some(parent.id.clone()),
        display_order: None,
        description: None,
        category_type,
    })
    .unwrap()
}

fn txn(
    date: &str,
    description: &str,
    amount: Decimal,
    currency: &str,
    category_id: &str,
    excluded: bool,
) -> ImportTransaction {
    ImportTransaction {
        date: NaiveDateTime::parse_from_str(date, "%Y-%m-%dT%H:%M:%S").unwrap(),
        description: description.to_string(),
        amount,
        currency: Some(currency.to_string()),
        category_id: Some(category_id.to_string()),
        category_source: None,
        notes: None,
        is_recurring: None,
        exclude_from_summary: if excluded { Some(true) } else { None },
        source_document_ids: Vec::new(),
    }
}

/// Parse a decimal string from a summary response and compare exactly
/// (Decimal equality ignores scale, so "65.0" == "65.00").
fn assert_dec(actual: &Value, expected: &str) {
    let actual = Decimal::from_str(actual.as_str().unwrap_or_default())
        .unwrap_or_else(|_| panic!("expected a decimal string, got {actual}"));
    assert_eq!(
        actual,
        Decimal::from_str(expected).unwrap(),
        "value {actual}"
    );
}

// ── CRUD ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn crud_round_trip_preserves_decimal_strings() {
    let (app, db_shared) = test_router();

    let created = emp(json!({
        "period_start": "2025-04-01",
        "period_end": "2025-04-30",
        "employees_ni": "110.50",
        "student_loan": "30.00",
        "net_pay": "659.50",
        "notes": "April payslip",
    }));
    let (status, row) = call(&app, Method::POST, "/api/employment-pay", Some(created)).await;
    assert_eq!(status, StatusCode::CREATED);
    let id = row["id"].as_str().unwrap().to_string();
    assert!(!id.is_empty());
    // Decimal-as-string round-trip: the exact scale posted must come back.
    assert_eq!(row["gross_pay"], "1000.00");
    assert_eq!(row["tax_deducted"], "200.00");
    assert_eq!(row["employees_ni"], "110.50");
    assert_eq!(row["net_pay"], "659.50");
    assert_eq!(row["currency"], "GBP");
    assert_eq!(row["is_p60"], false);
    assert_eq!(row["notes"], "April payslip");
    // created_at/updated_at are ISO strings.
    assert!(row["created_at"].as_str().unwrap().ends_with('Z'));

    let (status, list) = call(&app, Method::GET, "/api/employment-pay", None).await;
    assert_eq!(status, StatusCode::OK);
    let items = list["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], id);
    assert_eq!(items[0]["period_end"], "2025-04-30");

    // PATCH: set, clear (explicit null) and ignore-unknown in one call.
    let (status, patched) = call(
        &app,
        Method::PATCH,
        &format!("/api/employment-pay/{id}"),
        Some(json!({ "is_p60": true, "notes": null, "student_loan": "31.25", "bogus": 123 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(patched["is_p60"], true);
    assert!(
        patched.get("notes").is_none(),
        "explicit null must clear notes"
    );
    assert_eq!(patched["student_loan"], "31.25");
    // Untouched fields survive.
    assert_eq!(patched["gross_pay"], "1000.00");

    // PATCH null on a nullable decimal clears it.
    let (status, patched) = call(
        &app,
        Method::PATCH,
        &format!("/api/employment-pay/{id}"),
        Some(json!({ "employees_ni": null })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(patched.get("employees_ni").is_none());

    // Empty PATCH body and unknown id.
    let (status, body) = call(
        &app,
        Method::PATCH,
        &format!("/api/employment-pay/{id}"),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "empty_body");
    let (status, _) = call(
        &app,
        Method::PATCH,
        "/api/employment-pay/no-such-id",
        Some(json!({ "gross_pay": "1.00" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // DELETE is 204, then gone, then 404.
    let (status, _) = call(
        &app,
        Method::DELETE,
        &format!("/api/employment-pay/{id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, list) = call(&app, Method::GET, "/api/employment-pay", None).await;
    assert_eq!(list["items"].as_array().unwrap().len(), 0);
    let (status, _) = call(
        &app,
        Method::DELETE,
        &format!("/api/employment-pay/{id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // The direct-storage path sees the same empty table (shared handle).
    let db = db_shared.lock().unwrap();
    assert!(db.list_employment_pay(None).unwrap().is_empty());
}

#[tokio::test]
async fn list_filters_by_tax_year_and_sorts_period_end_nulls_last() {
    let (app, db_shared) = test_router();
    {
        let db = db_shared.lock().unwrap();
        seed_emp(
            &db,
            json!({ "tax_year": "2025-26", "employer": "Employer A", "period_end": "2025-05-31", "gross_pay": "500.00", "tax_deducted": "100.00" }),
        );
        seed_emp(
            &db,
            json!({ "tax_year": "2025-26", "employer": "Employer A", "period_end": "2025-04-30", "gross_pay": "500.00", "tax_deducted": "100.00" }),
        );
        seed_emp(
            &db,
            json!({ "tax_year": "2025-26", "employer": "Employer B", "gross_pay": "1500.00", "tax_deducted": "300.00", "is_p60": true }),
        );
        seed_emp(
            &db,
            json!({ "tax_year": "2024-25", "employer": "Employer A", "period_end": "2025-03-31", "gross_pay": "777.00", "tax_deducted": "111.00" }),
        );
    }

    let (status, list) = call(
        &app,
        Method::GET,
        "/api/employment-pay?tax_year=2025-26",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let items = list["items"].as_array().unwrap();
    assert_eq!(items.len(), 3);
    assert_eq!(items[0]["period_end"], "2025-04-30");
    assert_eq!(items[1]["period_end"], "2025-05-31");
    assert!(
        items[2].get("period_end").is_none(),
        "NULL period_end must sort last"
    );

    let (_, list) = call(&app, Method::GET, "/api/employment-pay", None).await;
    assert_eq!(list["items"].as_array().unwrap().len(), 4);
    let (_, list) = call(
        &app,
        Method::GET,
        "/api/employment-pay?tax_year=2024-25",
        None,
    )
    .await;
    assert_eq!(list["items"].as_array().unwrap().len(), 1);

    let (status, body) = call(
        &app,
        Method::GET,
        "/api/employment-pay?tax_year=2025-27",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "invalid_tax_year");
}

#[tokio::test]
async fn post_rejects_bad_payloads() {
    let (app, _db) = test_router();

    let cases: Vec<(Value, &str)> = vec![
        (emp(json!({ "tax_year": "2025-27" })), "invalid_tax_year"),
        (emp(json!({ "tax_year": "25-26x" })), "invalid_tax_year"),
        (emp(json!({ "employer": "   " })), "invalid_employer"),
        (emp(json!({ "period_start": "2025-13-01" })), "invalid_date"),
        (emp(json!({ "period_end": "not-a-date" })), "invalid_date"),
        (emp(json!({ "gross_pay": "-5.00" })), "negative_amount"),
        (emp(json!({ "tax_deducted": "-0.01" })), "negative_amount"),
        (emp(json!({ "student_loan": "-1" })), "negative_amount"),
        (
            emp(json!({ "transaction_id": "no-such-txn" })),
            "transaction_not_found",
        ),
    ];
    for (body, code) in cases {
        let (status, got) = post_emp(&app, body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "case {code}");
        assert_eq!(got["code"], code, "case {code}");
    }

    // A missing required money field is rejected by serde itself (axum
    // answers payload-shape failures with 422; the error body is not ours)
    // and must not create a row.
    let mut missing = emp_base();
    missing.as_object_mut().unwrap().remove("gross_pay");
    let (status, _) = post_emp(&app, missing).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (_, list) = call(&app, Method::GET, "/api/employment-pay", None).await;
    assert_eq!(list["items"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn transaction_link_requires_existing_transaction_and_survives_txn_delete() {
    let (app, db_shared) = test_router();
    let txn_id = {
        let db = db_shared.lock().unwrap();
        make_account(&db, "acct-pay", "Current Account");
        // Category names are globally UNIQUE and the default taxonomy is
        // seeded, so every test category gets a collision-proof prefix.
        let salary = leaf(&db, "Xt Pay", "Xt Salary", CategoryType::IncomeTaxable);
        db.insert_transactions_bulk(
            "acct-pay",
            &[txn(
                "2025-04-30T09:00:00",
                "Net pay credit",
                dec!(659.50),
                "GBP",
                &salary.id,
                false,
            )],
        )
        .unwrap();
        let (rows, _) = db.get_transactions(&TransactionFilters::default()).unwrap();
        rows[0].id.clone()
    };

    let (status, row) = post_emp(&app, emp(json!({ "transaction_id": txn_id.clone() }))).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(row["transaction_id"], txn_id);
    let id = row["id"].as_str().unwrap().to_string();

    // PATCH to a nonexistent transaction is also a 400.
    let (status, body) = call(
        &app,
        Method::PATCH,
        &format!("/api/employment-pay/{id}"),
        Some(json!({ "transaction_id": "no-such-txn" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "transaction_not_found");

    // Deleting the transaction nulls the link (ON DELETE SET NULL), not the row.
    {
        let db = db_shared.lock().unwrap();
        assert_eq!(db.delete_transaction(&txn_id).unwrap(), 1);
    }
    let (_, list) = call(&app, Method::GET, "/api/employment-pay", None).await;
    let items = list["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], id);
    assert!(items[0].get("transaction_id").is_none());
}

// ── Tax-year summary ─────────────────────────────────────────────────────────

#[tokio::test]
async fn summary_sums_employment_interest_and_donations() {
    let (app, db_shared) = test_router();
    {
        let db = db_shared.lock().unwrap();
        db.create_currency("USD", dec!(0.5)).unwrap();
        make_account(&db, "acct-uk", "Savings Account");
        make_account(&db, "acct-us", "USD Savings");
        let it_uk = leaf(
            &db,
            "Xt Interest UK",
            "Xt Savings interest",
            CategoryType::InterestTaxable,
        );
        let int_uk = leaf(
            &db,
            "Xt Interest UK exempt",
            "Xt ISA interest",
            CategoryType::InterestNonTaxable,
        );
        let gift = leaf(
            &db,
            "Xt Charity gift aid",
            "Xt Gift Aid giving",
            CategoryType::DonationTaxable,
        );
        let plain = leaf(
            &db,
            "Xt Charity other",
            "Xt Other giving",
            CategoryType::DonationNonTaxable,
        );
        let it_us = leaf(
            &db,
            "Xt Interest US",
            "Xt US savings",
            CategoryType::InterestTaxable,
        );
        let int_us = leaf(
            &db,
            "Xt Interest US exempt",
            "Xt US ISA",
            CategoryType::InterestNonTaxable,
        );

        db.insert_transactions_bulk(
            "acct-uk",
            &[
                // First and last days of the 2025-26 window are in range.
                txn(
                    "2025-04-06T00:00:00",
                    "Interest",
                    dec!(10.50),
                    "GBP",
                    &it_uk.id,
                    false,
                ),
                txn(
                    "2026-04-05T23:30:00",
                    "Interest",
                    dec!(4.50),
                    "GBP",
                    &it_uk.id,
                    false,
                ),
                // One day either side belong to the neighbouring tax years.
                txn(
                    "2025-04-05T23:59:59",
                    "Old year interest",
                    dec!(999.99),
                    "GBP",
                    &it_uk.id,
                    false,
                ),
                txn(
                    "2026-04-06T00:00:00",
                    "Next year interest",
                    dec!(999.99),
                    "GBP",
                    &it_uk.id,
                    false,
                ),
                txn(
                    "2025-07-01T12:00:00",
                    "ISA interest",
                    dec!(8.50),
                    "GBP",
                    &int_uk.id,
                    false,
                ),
                // Excluded rows must not reach any total.
                txn(
                    "2025-09-01T12:00:00",
                    "Excluded interest",
                    dec!(1000.00),
                    "GBP",
                    &it_uk.id,
                    true,
                ),
                // Donations: outflows count, a refund leg does not.
                txn(
                    "2025-05-10T12:00:00",
                    "Gift Aid",
                    dec!(-50.00),
                    "GBP",
                    &gift.id,
                    false,
                ),
                txn(
                    "2025-11-02T12:00:00",
                    "Gift Aid",
                    dec!(-12.34),
                    "GBP",
                    &gift.id,
                    false,
                ),
                txn(
                    "2025-11-03T12:00:00",
                    "Gift Aid refund",
                    dec!(5.00),
                    "GBP",
                    &gift.id,
                    false,
                ),
                txn(
                    "2025-08-15T12:00:00",
                    "Other giving",
                    dec!(-20.00),
                    "GBP",
                    &plain.id,
                    false,
                ),
                txn(
                    "2025-08-16T12:00:00",
                    "Excluded giving",
                    dec!(-999.99),
                    "GBP",
                    &plain.id,
                    true,
                ),
            ],
        )
        .unwrap();
        db.insert_transactions_bulk(
            "acct-us",
            &[
                txn(
                    "2025-06-01T12:00:00",
                    "US interest",
                    dec!(100.00),
                    "USD",
                    &it_us.id,
                    false,
                ),
                txn(
                    "2025-06-02T12:00:00",
                    "US ISA",
                    dec!(10.00),
                    "USD",
                    &int_us.id,
                    false,
                ),
            ],
        )
        .unwrap();

        // Employment: two payslips and one P60, in the right and wrong years.
        seed_emp(
            &db,
            json!({ "tax_year": "2025-26", "employer": "Employer A", "period_start": "2025-04-01", "period_end": "2025-04-30", "gross_pay": "1000.00", "tax_deducted": "200.00", "employees_ni": "110.50", "student_loan": "30.00", "net_pay": "659.50" }),
        );
        seed_emp(
            &db,
            json!({ "tax_year": "2025-26", "employer": "Employer A", "period_start": "2025-05-01", "period_end": "2025-05-31", "gross_pay": "500.00", "tax_deducted": "100.00", "employees_ni": "0" }),
        );
        seed_emp(
            &db,
            json!({ "tax_year": "2025-26", "employer": "Employer A", "gross_pay": "1500.00", "tax_deducted": "300.00", "is_p60": true }),
        );
        seed_emp(
            &db,
            json!({ "tax_year": "2024-25", "employer": "Employer A", "gross_pay": "777.00", "tax_deducted": "111.00" }),
        );
    }

    let (status, s) = call(&app, Method::GET, "/api/tax/summary/2025-26", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(s["tax_year"], "2025-26");
    assert_eq!(s["start_date"], "2025-04-06");
    assert_eq!(s["end_date"], "2026-04-05");

    // Employment: the P60 adds its full-year figures once; NULL legs are 0
    // but flag the gap.
    let e = &s["employment"];
    assert_dec(&e["gross_pay"], "3000.00");
    assert_dec(&e["tax_deducted"], "600.00");
    assert_dec(&e["employees_ni"], "110.50");
    assert_dec(&e["student_loan"], "30.00");
    assert_dec(&e["net_pay"], "659.50");
    assert_eq!(e["payslip_count"], 3);
    assert_eq!(e["has_p60"], true);
    assert_eq!(e["has_unknown_employees_ni"], true);
    assert_eq!(e["has_unknown_student_loan"], true);
    assert_eq!(e["has_unknown_net_pay"], true);

    // Interest: window-bounded, per account, FX-converted at the flat rate
    // (USD 0.5): 10.50 + 4.50 + 100 * 0.5 = 65.00 taxable.
    let i = &s["interest"];
    assert_dec(&i["taxable_total"], "65.00");
    assert_dec(&i["non_taxable_total"], "13.50");
    let by = i["by_account"].as_array().unwrap();
    assert_eq!(by.len(), 2);
    assert_eq!(by[0]["account_name"], "Savings Account");
    assert_dec(&by[0]["taxable"], "15.00");
    assert_dec(&by[0]["non_taxable"], "8.50");
    assert_eq!(by[1]["account_name"], "USD Savings");
    assert_dec(&by[1]["taxable"], "50.00");
    assert_dec(&by[1]["non_taxable"], "5.00");

    // Donations: outflow legs only, split by Gift Aid flag.
    assert_dec(&s["donations"]["gift_aid_total"], "62.34");
    assert_dec(&s["donations"]["non_gift_aid_total"], "20.00");

    // A year with only one employment row and no transactions still answers.
    let (status, prev) = call(&app, Method::GET, "/api/tax/summary/2024-25", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_dec(&prev["employment"]["gross_pay"], "777.00");
    assert_eq!(prev["employment"]["payslip_count"], 1);
    assert_eq!(prev["employment"]["has_p60"], false);
    // The 5 April boundary row is the *previous* year's last day, so it
    // shows up here and nowhere in 2025-26.
    let prev_by = prev["interest"]["by_account"].as_array().unwrap();
    assert_eq!(prev_by.len(), 1);
    assert_dec(&prev_by[0]["taxable"], "999.99");
    assert_dec(&prev["interest"]["taxable_total"], "999.99");
    assert_dec(&prev["donations"]["gift_aid_total"], "0");

    // Bad tax_year on the summary route is a 400, not an empty year.
    let (status, body) = call(&app, Method::GET, "/api/tax/summary/2025-27", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "invalid_tax_year");
}

#[tokio::test]
async fn summary_treats_recorded_zero_as_known_not_missing() {
    let (app, db_shared) = test_router();
    {
        let db = db_shared.lock().unwrap();
        seed_emp(
            &db,
            json!({ "tax_year": "2026-27", "employer": "Employer A", "gross_pay": "100.00", "tax_deducted": "10.00", "employees_ni": "0.00", "student_loan": "0.00", "net_pay": "90.00" }),
        );
        seed_emp(
            &db,
            json!({ "tax_year": "2026-27", "employer": "Employer B", "gross_pay": "200.00", "tax_deducted": "20.00", "employees_ni": "10.00" }),
        );
    }

    let (status, s) = call(&app, Method::GET, "/api/tax/summary/2026-27", None).await;
    assert_eq!(status, StatusCode::OK);
    let e = &s["employment"];
    // An explicit 0 is recorded, not unknown: the NI flag stays false even
    // though one row deducted nothing.
    assert_eq!(e["has_unknown_employees_ni"], false);
    assert_dec(&e["employees_ni"], "10.00");
    // The other two legs genuinely have a NULL row.
    assert_eq!(e["has_unknown_student_loan"], true);
    assert_eq!(e["has_unknown_net_pay"], true);
    assert_dec(&e["student_loan"], "0.00");
    assert_dec(&e["net_pay"], "90.00");
}
