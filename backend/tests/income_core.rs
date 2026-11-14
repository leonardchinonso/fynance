//! The income model (plan 30): sources, records, checks, the year-end
//! adjustment, RSU vest candidates, reference integrity on delete, and the
//! `GET /api/tax/summary/:tax_year` figures. CRUD goes through the router so
//! status codes and error codes are pinned; seed data goes straight to
//! storage. All data synthetic.

use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode};
use chrono::NaiveDateTime;
use fynance::model::{
    Account, AccountType, CategoryType, CreateCategoryPayload, CreateInvestmentEventBody,
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
    let path = dir.keep().join("income_core_test.db");
    let db = Db::open(&path).unwrap();
    db.create_profile("alex", "Alex").unwrap();
    db.create_profile("sam", "Sam").unwrap();
    let shared = Arc::new(Mutex::new(db));
    (build_router(shared.clone(), true), shared)
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

fn assert_dec(actual: &Value, expected: &str) {
    let actual = Decimal::from_str(actual.as_str().unwrap_or_default())
        .unwrap_or_else(|_| panic!("expected a decimal string, got {actual}"));
    assert_eq!(
        actual,
        Decimal::from_str(expected).unwrap(),
        "value {actual}"
    );
}

async fn make_source(app: &axum::Router, profile: &str, kind: &str, name: &str) -> String {
    let (status, body) = call(
        app,
        Method::POST,
        "/api/income-sources",
        Some(json!({ "profile_id": profile, "source_type": kind, "name": name })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["id"].as_str().unwrap().to_string()
}

fn payslip(source_id: &str, start: &str, end: &str, pay_date: &str, lines: Value) -> Value {
    json!({
        "source_id": source_id,
        "period_start": start,
        "period_end": end,
        "pay_date": pay_date,
        "line_items": lines,
    })
}

async fn post_record(app: &axum::Router, body: Value) -> (StatusCode, Value) {
    call(app, Method::POST, "/api/income-records", Some(body)).await
}

async fn summary(app: &axum::Router, profile: &str, year: &str) -> Value {
    let (status, body) = call(
        app,
        Method::GET,
        &format!("/api/tax/summary/{year}?profile_id={profile}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

fn make_account(db: &Db, id: &str, name: &str, profiles: &[&str]) {
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
        profile_ids: profiles.iter().map(|p| p.to_string()).collect(),
        is_stale: None,
        is_available: true,
    })
    .unwrap();
}

fn leaf(db: &Db, parent: &str, name: &str, ty: CategoryType) -> fynance::model::Category {
    let p = db
        .create_category(&CreateCategoryPayload {
            name: parent.to_string(),
            parent_id: None,
            display_order: None,
            description: None,
            category_type: ty,
        })
        .unwrap();
    db.create_category(&CreateCategoryPayload {
        name: name.to_string(),
        parent_id: Some(p.id.clone()),
        display_order: None,
        description: None,
        category_type: ty,
    })
    .unwrap()
}

fn txn(date: &str, description: &str, amount: Decimal, category_id: &str) -> ImportTransaction {
    ImportTransaction {
        date: NaiveDateTime::parse_from_str(date, "%Y-%m-%dT%H:%M:%S").unwrap(),
        description: description.to_string(),
        amount,
        currency: Some("GBP".to_string()),
        category_id: Some(category_id.to_string()),
        category_source: None,
        notes: None,
        is_recurring: None,
        exclude_from_summary: None,
        source_document_ids: Vec::new(),
    }
}

// ── Catalogue and sources ────────────────────────────────────────────────────

#[tokio::test]
async fn line_type_catalogue_is_served() {
    let (app, _db) = test_router();
    let (status, body) = call(&app, Method::GET, "/api/income/line-types", None).await;
    assert_eq!(status, StatusCode::OK);
    let items = body["items"].as_array().unwrap();
    let paye = items.iter().find(|i| i["line_type"] == "paye_tax").unwrap();
    assert_eq!(paye["form"], "SA102");
    assert_eq!(paye["summed"], true);
    let costing = items
        .iter()
        .find(|i| i["line_type"] == "costing_materials")
        .unwrap();
    assert_eq!(costing["summed"], false);
}

#[tokio::test]
async fn source_crud_and_delete_guard() {
    let (app, _db) = test_router();
    let id = make_source(&app, "alex", "employment", "Employer A").await;

    let (status, list) = call(
        &app,
        Method::GET,
        "/api/income-sources?profile_id=alex",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["items"].as_array().unwrap().len(), 1);
    let (_, other) = call(
        &app,
        Method::GET,
        "/api/income-sources?profile_id=sam",
        None,
    )
    .await;
    assert!(other["items"].as_array().unwrap().is_empty());

    let (status, patched) = call(
        &app,
        Method::PATCH,
        &format!("/api/income-sources/{id}"),
        Some(json!({ "identifiers": { "paye_reference": "123/AB456" }, "notes": "n" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(patched["identifiers"]["paye_reference"], "123/AB456");
    assert_eq!(patched["notes"], "n");
    // An explicit null clears a nullable field.
    let (_, cleared) = call(
        &app,
        Method::PATCH,
        &format!("/api/income-sources/{id}"),
        Some(json!({ "notes": null })),
    )
    .await;
    assert!(cleared.get("notes").is_none());

    let (status, body) = call(
        &app,
        Method::PATCH,
        &format!("/api/income-sources/{id}"),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "empty_body");

    // Business-only fields are refused on an employment source.
    let (status, body) = call(
        &app,
        Method::PATCH,
        &format!("/api/income-sources/{id}"),
        Some(json!({ "sync_kind": "shop_app" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "invalid_source_field");

    let (status, _) = post_record(
        &app,
        payslip(
            &id,
            "2025-04-01",
            "2025-04-30",
            "2025-04-28",
            json!([{ "line_type": "taxable_pay", "amount": "100" }]),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, body) = call(
        &app,
        Method::DELETE,
        &format!("/api/income-sources/{id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "source_has_records");
}

// ── Records: tax year and validation ─────────────────────────────────────────

#[tokio::test]
async fn employment_tax_year_follows_pay_date_across_5_and_6_april() {
    let (app, _db) = test_router();
    let id = make_source(&app, "alex", "employment", "Employer A").await;
    let lines = json!([{ "line_type": "taxable_pay", "amount": "100.50" }]);

    // The period straddles the boundary; PAYE is by payment date.
    let (status, a) = post_record(
        &app,
        payslip(&id, "2025-03-06", "2025-04-05", "2025-04-05", lines.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{a}");
    assert_eq!(a["tax_year"], "2024-25");
    let (status, b) = post_record(
        &app,
        payslip(&id, "2025-03-06", "2025-04-05", "2025-04-06", lines.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{b}");
    assert_eq!(b["tax_year"], "2025-26");
    // Without a pay date the period end decides.
    let (_, c) = post_record(
        &app,
        json!({ "source_id": id, "period_start": "2025-03-07", "period_end": "2025-04-06",
                "line_items": lines }),
    )
    .await;
    assert_eq!(c["tax_year"], "2025-26");
    // Decimal strings round-trip at the scale posted.
    assert_eq!(a["line_items"][0]["amount"], "100.50");
}

#[tokio::test]
async fn record_validation_rejects_bad_input() {
    let (app, db_shared) = test_router();
    let emp = make_source(&app, "alex", "employment", "Employer A").await;
    let biz = make_source(&app, "alex", "self_employment", "Business A").await;
    let ok_line = json!([{ "line_type": "taxable_pay", "amount": "1" }]);

    let cases: Vec<(Value, &str)> = vec![
        (
            payslip(
                "nope",
                "2025-04-01",
                "2025-04-30",
                "2025-04-30",
                ok_line.clone(),
            ),
            "source_not_found",
        ),
        (
            payslip(
                &emp,
                "2025-04-xx",
                "2025-04-30",
                "2025-04-30",
                ok_line.clone(),
            ),
            "invalid_date",
        ),
        (
            payslip(
                &emp,
                "2025-04-30",
                "2025-04-01",
                "2025-04-30",
                ok_line.clone(),
            ),
            "invalid_period",
        ),
        (
            payslip(
                &emp,
                "2025-04-01",
                "2025-04-30",
                "2025-04-30",
                json!([{ "line_type": "taxable_pay", "amount": "-1" }]),
            ),
            "negative_amount",
        ),
        (
            payslip(
                &emp,
                "2025-04-01",
                "2025-04-30",
                "2025-04-30",
                json!([{ "line_type": "sales", "amount": "1" }]),
            ),
            "invalid_line_type",
        ),
        (
            payslip(
                &emp,
                "2025-04-01",
                "2025-04-30",
                "2025-04-30",
                json!([{ "line_type": "rsu_vest_value", "amount": "1", "investment_event_id": "nope" }]),
            ),
            "invalid_investment_event",
        ),
        (
            json!({ "source_id": emp, "period_start": "2025-04-01", "period_end": "2025-04-30",
                    "transaction_ids": ["nope"], "line_items": ok_line }),
            "transaction_not_found",
        ),
        (
            json!({ "source_id": emp, "period_start": "2025-04-01", "period_end": "2025-04-30",
                    "currency": "ZZZ", "line_items": ok_line }),
            "invalid_currency",
        ),
        // A business period may not straddle 5/6 April.
        (
            json!({ "source_id": biz, "period_start": "2025-04-01", "period_end": "2025-04-30",
                    "line_items": [{ "line_type": "sales", "amount": "1" }] }),
            "period_crosses_tax_year",
        ),
    ];
    for (body, code) in cases {
        let (status, resp) = post_record(&app, body.clone()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{code}: {resp}");
        assert_eq!(resp["code"], code, "{body}");
    }
    drop(db_shared);
}

#[tokio::test]
async fn patch_replaces_line_items_only_when_present() {
    let (app, _db) = test_router();
    let id = make_source(&app, "alex", "employment", "Employer A").await;
    let (_, rec) = post_record(
        &app,
        payslip(
            &id,
            "2025-04-01",
            "2025-04-30",
            "2025-04-28",
            json!([{ "line_type": "taxable_pay", "amount": "100" }]),
        ),
    )
    .await;
    let rid = rec["id"].as_str().unwrap();
    let line_id = rec["line_items"][0]["id"].clone();

    let (status, p1) = call(
        &app,
        Method::PATCH,
        &format!("/api/income-records/{rid}"),
        Some(json!({ "notes": "n" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(p1["line_items"][0]["id"], line_id);

    let (_, p2) = call(
        &app,
        Method::PATCH,
        &format!("/api/income-records/{rid}"),
        Some(json!({ "line_items": [{ "line_type": "paye_tax", "amount": "20" }] })),
    )
    .await;
    assert_eq!(p2["line_items"].as_array().unwrap().len(), 1);
    assert_eq!(p2["line_items"][0]["line_type"], "paye_tax");

    // Moving the pay date across the boundary re-derives the tax year.
    let (_, p3) = call(
        &app,
        Method::PATCH,
        &format!("/api/income-records/{rid}"),
        Some(
            json!({ "pay_date": "2026-04-06", "period_end": "2026-04-06",
                     "period_start": "2026-04-01" }),
        ),
    )
    .await;
    assert_eq!(p3["tax_year"], "2026-27");

    let (status, _) = call(
        &app,
        Method::DELETE,
        &format!("/api/income-records/{rid}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = call(
        &app,
        Method::GET,
        &format!("/api/income-records/{rid}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ── Summary: employment ──────────────────────────────────────────────────────

#[tokio::test]
async fn summary_uses_taxable_pay_ignores_ytd_and_never_sums_the_p60() {
    let (app, _db) = test_router();
    let id = make_source(&app, "alex", "employment", "Employer A").await;

    // Month 1: taxable pay below gross (salary sacrifice); gross is only shown.
    post_record(
        &app,
        payslip(
            &id,
            "2025-04-01",
            "2025-04-30",
            "2025-04-28",
            json!([
                { "line_type": "gross_pay", "amount": "3000" },
                { "line_type": "taxable_pay", "amount": "2800" },
                { "line_type": "paye_tax", "amount": "400" },
                { "line_type": "taxable_pay", "amount": "2800", "basis": "year_to_date" },
                { "line_type": "paye_tax", "amount": "400", "basis": "year_to_date" },
                { "line_type": "employee_ni", "amount": "200" },
                { "line_type": "student_loan", "amount": "50" },
            ]),
        ),
    )
    .await;
    // Month 2: no taxable pay line, so gross counts.
    post_record(
        &app,
        payslip(
            &id,
            "2025-05-01",
            "2025-05-31",
            "2025-05-28",
            json!([
                { "line_type": "gross_pay", "amount": "3000" },
                { "line_type": "paye_tax", "amount": "450" },
                { "line_type": "rsu_vest_value", "amount": "500" },
                { "line_type": "rsu_tax", "amount": "200" },
            ]),
        ),
    )
    .await;
    // A P60 is a check, never a record: stored, but not summed.
    let (status, check) = call(
        &app,
        Method::POST,
        "/api/income-checks",
        Some(
            json!({ "source_id": id, "tax_year": "2025-26", "check_type": "p60",
                     "figures": [
                        { "line_type": "taxable_pay", "amount": "5800", "currency": "GBP" },
                        { "line_type": "paye_tax", "amount": "850", "currency": "GBP" } ] }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{check}");

    let s = summary(&app, "alex", "2025-26").await;
    assert_eq!(s["profile_id"], "alex");
    let emp = &s["employment"][0];
    assert_eq!(emp["record_count"], 2);
    assert_eq!(emp["months_covered"], json!(["2025-04", "2025-05"]));
    assert_dec(&emp["boxes"]["pay"], "5800");
    assert_dec(&emp["boxes"]["tax"], "850");
    assert_dec(&emp["boxes"]["student_loan"], "50");
    assert_dec(&emp["shown"]["gross_pay"], "6000");
    assert_dec(&emp["shown"]["employee_ni"], "200");
    // RSU lines are shown, never added to the boxes.
    assert_dec(&emp["shown"]["rsu_vest_value"], "500");
    assert_dec(&emp["shown"]["rsu_tax"], "200");
    assert_eq!(emp["check"]["status"], "matched");
    assert_dec(&emp["check"]["pay_difference"], "0");
    assert_eq!(emp["rsu"][0]["status"], "unlinked");
    assert_eq!(s["cgt_link"]["start_date"], "2025-04-06");

    // Another profile sees nothing of it.
    let other = summary(&app, "sam", "2025-26").await;
    assert!(other["employment"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn gap_becomes_a_labelled_adjustment_and_check_reads_adjusted() {
    let (app, _db) = test_router();
    let id = make_source(&app, "alex", "employment", "Employer A").await;
    let adj = format!("/api/income-sources/{id}/year-end-adjustment");

    let (status, body) = call(
        &app,
        Method::POST,
        &adj,
        Some(json!({ "tax_year": "2025-26" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "no_check");

    post_record(
        &app,
        payslip(
            &id,
            "2025-04-01",
            "2025-04-30",
            "2025-04-28",
            json!([
                { "line_type": "taxable_pay", "amount": "1000" },
                { "line_type": "paye_tax", "amount": "200" },
            ]),
        ),
    )
    .await;
    let (_, check) = call(
        &app,
        Method::POST,
        "/api/income-checks",
        Some(
            json!({ "source_id": id, "tax_year": "2025-26", "check_type": "p60",
                     "figures": [
                        { "line_type": "taxable_pay", "amount": "1500", "currency": "GBP" },
                        { "line_type": "paye_tax", "amount": "150", "currency": "GBP" } ] }),
        ),
    )
    .await;
    // A second check for the same source, year and type clashes.
    let (status, body) = call(
        &app,
        Method::POST,
        "/api/income-checks",
        Some(json!({ "source_id": id, "tax_year": "2025-26", "check_type": "p60", "figures": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "check_exists");

    let s = summary(&app, "alex", "2025-26").await;
    assert_eq!(s["employment"][0]["check"]["status"], "gap");
    assert_dec(&s["employment"][0]["check"]["pay_difference"], "500");
    assert_dec(&s["employment"][0]["check"]["tax_difference"], "-50");

    let (status, record) = call(
        &app,
        Method::POST,
        &adj,
        Some(json!({ "tax_year": "2025-26" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{record}");
    assert_eq!(record["kind"], "year_end_adjustment");
    assert_eq!(record["origin"], "adjustment");
    assert_eq!(record["period_start"], "2025-04-06");
    assert_eq!(record["period_end"], "2026-04-05");
    let lines = record["line_items"].as_array().unwrap();
    let pay = lines
        .iter()
        .find(|l| l["line_type"] == "taxable_pay")
        .unwrap();
    let tax = lines.iter().find(|l| l["line_type"] == "paye_tax").unwrap();
    assert_dec(&pay["amount"], "500");
    assert_dec(&tax["amount"], "-50");

    let s = summary(&app, "alex", "2025-26").await;
    let emp = &s["employment"][0];
    assert_eq!(emp["check"]["status"], "adjusted");
    assert_dec(&emp["boxes"]["pay"], "1500");
    assert_dec(&emp["boxes"]["tax"], "150");
    // The adjustment is not a payslip.
    assert_eq!(emp["record_count"], 1);

    let (status, body) = call(
        &app,
        Method::POST,
        &adj,
        Some(json!({ "tax_year": "2025-26" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "adjustment_exists");

    // Negative amounts are accepted on adjustments only.
    let (status, _) = post_record(
        &app,
        payslip(
            &id,
            "2025-05-01",
            "2025-05-31",
            "2025-05-28",
            json!([{ "line_type": "paye_tax", "amount": "-5" }]),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // A matching check has no gap to adjust.
    let id2 = make_source(&app, "sam", "employment", "Employer B").await;
    post_record(
        &app,
        payslip(
            &id2,
            "2025-04-01",
            "2025-04-30",
            "2025-04-28",
            json!([{ "line_type": "taxable_pay", "amount": "100" }]),
        ),
    )
    .await;
    call(
        &app,
        Method::POST,
        "/api/income-checks",
        Some(json!({ "source_id": id2, "tax_year": "2025-26", "check_type": "p60",
                     "figures": [{ "line_type": "taxable_pay", "amount": "100", "currency": "GBP" }] })),
    )
    .await;
    let (status, body) = call(
        &app,
        Method::POST,
        &format!("/api/income-sources/{id2}/year-end-adjustment"),
        Some(json!({ "tax_year": "2025-26" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "no_gap");
    drop(check);
}

#[tokio::test]
async fn summary_requires_a_known_profile() {
    let (app, _db) = test_router();
    let (status, body) = call(&app, Method::GET, "/api/tax/summary/2025-26", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "missing_profile_id");
    let (status, body) = call(
        &app,
        Method::GET,
        "/api/tax/summary/2025-26?profile_id=ghost",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "profile_not_found");
    let (status, body) = call(
        &app,
        Method::GET,
        "/api/tax/summary/2025-27?profile_id=alex",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "invalid_tax_year");
}

// ── Summary: self-employment ─────────────────────────────────────────────────

#[tokio::test]
async fn self_employment_sums_records_and_mapped_bank_outflows() {
    let (app, db_shared) = test_router();
    let (cat_materials, cat_other) = {
        let db = db_shared.lock().unwrap();
        make_account(&db, "biz-acct", "Business Current", &["alex"]);
        let materials = leaf(&db, "Xt Biz", "Xt Materials", CategoryType::Spending);
        let other = leaf(&db, "Xt Biz misc", "Xt Misc", CategoryType::Spending);
        let transfer = leaf(
            &db,
            "Xt Biz xfer",
            "Xt Xfer",
            CategoryType::InternalTransfer,
        );
        db.insert_transactions_bulk(
            "biz-acct",
            &[
                txn(
                    "2025-05-10T10:00:00",
                    "Supplier",
                    dec!(-120.40),
                    &materials.id,
                ),
                txn("2025-05-11T10:00:00", "Supplier", dec!(-30), &materials.id),
                txn("2025-05-12T10:00:00", "Sundry", dec!(-12), &other.id),
                // A transfer between own accounts is not an expense.
                txn(
                    "2025-05-13T10:00:00",
                    "To savings",
                    dec!(-500),
                    &transfer.id,
                ),
                // Money in is not an expense.
                txn("2025-05-14T10:00:00", "Refund", dec!(40), &materials.id),
                // Outside the tax year.
                txn("2025-04-01T10:00:00", "Old", dec!(-99), &materials.id),
            ],
        )
        .unwrap();
        (materials.id, other.id)
    };

    let (status, biz) = call(
        &app,
        Method::POST,
        "/api/income-sources",
        Some(
            json!({ "profile_id": "alex", "source_type": "self_employment",
                     "name": "Business A", "account_ids": ["biz-acct"],
                     "expense_category_map": { cat_materials.clone(): "cost_of_goods" } }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{biz}");
    let id = biz["id"].as_str().unwrap();

    let (status, body) = call(
        &app,
        Method::PATCH,
        &format!("/api/income-sources/{id}"),
        Some(json!({ "expense_category_map": { cat_materials.clone(): "sales" } })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "invalid_line_type");

    for (start, end, sales, refunds) in [
        ("2025-04-06", "2025-04-30", "700", "0"),
        ("2025-05-01", "2025-05-31", "900", "100"),
    ] {
        let (status, rec) = post_record(
            &app,
            json!({ "source_id": id, "period_start": start, "period_end": end,
                    "line_items": [
                        { "line_type": "sales", "amount": sales },
                        { "line_type": "refunds", "amount": refunds },
                        { "line_type": "platform_fees", "amount": "25" },
                        { "line_type": "costing_materials", "amount": "300" },
                    ] }),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{rec}");
    }

    let s = summary(&app, "alex", "2025-26").await;
    let biz = &s["self_employment"][0];
    assert_dec(&biz["turnover"], "1500");
    // Materials 150.40 mapped from the bank plus 50 platform fees; the unmapped
    // category is listed, not summed; costings are information only.
    assert_dec(&biz["expenses_by_line_type"]["cost_of_goods"], "150.40");
    assert_dec(&biz["expenses_by_line_type"]["platform_fees"], "50");
    assert_dec(&biz["total_expenses"], "200.40");
    assert_dec(&biz["net_profit"], "1299.60");
    assert_dec(&biz["bank_expenses"]["mapped_total"], "150.40");
    let unmapped = biz["bank_expenses"]["unmapped"].as_array().unwrap();
    assert_eq!(unmapped.len(), 1);
    assert_eq!(unmapped[0]["category_id"], cat_other);
    assert_dec(&unmapped[0]["total"], "12");
    assert_dec(&biz["info"]["costing_materials"], "600");
    assert_eq!(biz["trading_allowance_better"], true);
    assert_eq!(biz["months_covered"], json!(["2025-04", "2025-05"]));
}

// ── Summary: interest and Gift Aid per profile ───────────────────────────────

#[tokio::test]
async fn joint_account_interest_is_split_between_its_profiles() {
    let (app, db_shared) = test_router();
    {
        let db = db_shared.lock().unwrap();
        make_account(&db, "solo", "Solo Savings", &["alex"]);
        make_account(&db, "joint", "Joint Savings", &["alex", "sam"]);
        let interest = leaf(&db, "Xt Int", "Xt Interest", CategoryType::InterestTaxable);
        let gift = leaf(&db, "Xt Gift", "Xt Giving", CategoryType::DonationTaxable);
        db.insert_transactions_bulk(
            "solo",
            &[txn(
                "2025-06-01T10:00:00",
                "Interest",
                dec!(10),
                &interest.id,
            )],
        )
        .unwrap();
        db.insert_transactions_bulk(
            "joint",
            &[
                txn("2025-06-01T10:00:00", "Interest", dec!(40.02), &interest.id),
                txn("2025-06-02T10:00:00", "Charity", dec!(-20), &gift.id),
            ],
        )
        .unwrap();
    }
    let alex = summary(&app, "alex", "2025-26").await;
    assert_dec(&alex["interest"]["taxable_total"], "30.01");
    assert_dec(&alex["donations"]["gift_aid_total"], "10");
    let joint = alex["interest"]["by_account"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["account_id"] == "joint")
        .unwrap();
    assert_eq!(joint["joint_share_of"], 2);
    let sam = summary(&app, "sam", "2025-26").await;
    assert_dec(&sam["interest"]["taxable_total"], "20.01");
}

// ── RSU vest linking ─────────────────────────────────────────────────────────

#[tokio::test]
async fn vest_candidates_match_by_value_and_link_survives_only_vest_events() {
    let (app, db_shared) = test_router();
    let (vest_id, buy_id) = {
        let db = db_shared.lock().unwrap();
        make_account(&db, "broker", "Broker A", &["alex"]);
        let mk = |kind: &str, date: &str| CreateInvestmentEventBody {
            account_id: "broker".to_string(),
            event_type: kind.to_string(),
            symbol: "ACME".to_string(),
            date: date.to_string(),
            quantity: "10".to_string(),
            price_per_share: "50".to_string(),
            fee: None,
            currency: "GBP".to_string(),
            fee_currency: None,
            notes: None,
            source_document_ids: Vec::new(),
        };
        let (vest, _) = db
            .create_investment_event(&mk("vest", "2025-05-26T00:00:00"))
            .unwrap();
        let (buy, _) = db
            .create_investment_event(&mk("buy", "2025-05-27T00:00:00"))
            .unwrap();
        // A vest far from the pay date is not a candidate.
        db.create_investment_event(&mk("vest", "2025-08-01T00:00:00"))
            .unwrap();
        (vest.id, buy.id)
    };
    let id = make_source(&app, "alex", "employment", "Employer A").await;

    // A buy event cannot be linked as a vest.
    let (status, body) = post_record(
        &app,
        payslip(
            &id,
            "2025-05-01",
            "2025-05-31",
            "2025-05-28",
            json!([{ "line_type": "rsu_vest_value", "amount": "500", "investment_event_id": buy_id }]),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "invalid_investment_event");

    let (status, rec) = post_record(
        &app,
        payslip(
            &id,
            "2025-05-01",
            "2025-05-31",
            "2025-05-28",
            json!([{ "line_type": "rsu_vest_value", "amount": "500.50", "investment_event_id": vest_id }]),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{rec}");
    let rid = rec["id"].as_str().unwrap();

    let (status, cands) = call(
        &app,
        Method::GET,
        &format!("/api/income/vest-candidates?record_id={rid}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let items = cands["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["investment_event_id"], vest_id);
    assert_eq!(items[0]["status"], "matched");
    assert_dec(&items[0]["value_gbp"], "500");

    let s = summary(&app, "alex", "2025-26").await;
    assert_eq!(s["employment"][0]["rsu"][0]["status"], "matched");

    // A payslip value more than a pound away reads as a mismatch.
    let (_, rec2) = post_record(
        &app,
        payslip(
            &id,
            "2025-05-01",
            "2025-05-31",
            "2025-05-29",
            json!([{ "line_type": "rsu_vest_value", "amount": "520", "investment_event_id": vest_id }]),
        ),
    )
    .await;
    let rid2 = rec2["id"].as_str().unwrap();
    let (_, cands2) = call(
        &app,
        Method::GET,
        &format!("/api/income/vest-candidates?record_id={rid2}"),
        None,
    )
    .await;
    assert_eq!(cands2["items"][0]["status"], "mismatch");

    // Deleting the event unlinks the line item rather than the record.
    {
        let db = db_shared.lock().unwrap();
        assert!(db.delete_investment_event(&vest_id).unwrap());
    }
    let (_, after) = call(
        &app,
        Method::GET,
        &format!("/api/income-records/{rid}"),
        None,
    )
    .await;
    assert!(after["line_items"][0].get("investment_event_id").is_none());
}

// ── Reference integrity ──────────────────────────────────────────────────────

#[tokio::test]
async fn deleting_a_transaction_or_document_strips_it_from_income_records() {
    let (app, db_shared) = test_router();
    let (tx_id, doc_id) = {
        let db = db_shared.lock().unwrap();
        make_account(&db, "acct", "Current", &["alex"]);
        let salary = leaf(&db, "Xt Pay", "Xt Salary", CategoryType::IncomeTaxable);
        db.insert_transactions_bulk(
            "acct",
            &[txn("2025-04-28T09:00:00", "Net pay", dec!(660), &salary.id)],
        )
        .unwrap();
        let (rows, _) = db.get_transactions(&TransactionFilters::default()).unwrap();
        let (doc, _) = db
            .store_document(
                "payslip.txt",
                "text/plain",
                b"synthetic payslip",
                "test",
                None,
            )
            .unwrap();
        (rows[0].id.clone(), doc.id)
    };
    let id = make_source(&app, "alex", "employment", "Employer A").await;
    let (status, rec) = post_record(
        &app,
        json!({ "source_id": id, "period_start": "2025-04-01", "period_end": "2025-04-30",
                "pay_date": "2025-04-28", "transaction_ids": [tx_id], "document_ids": [doc_id],
                "line_items": [{ "line_type": "net_pay", "amount": "660" }] }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{rec}");
    let rid = rec["id"].as_str().unwrap();
    call(
        &app,
        Method::POST,
        "/api/income-checks",
        Some(
            json!({ "source_id": id, "tax_year": "2025-26", "check_type": "p60",
                     "figures": [], "document_ids": [doc_id] }),
        ),
    )
    .await;

    // The document is referenced by the record and the check; no force, no delete.
    let (status, body) = call(
        &app,
        Method::DELETE,
        &format!("/api/documents/{doc_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    let refs = db_shared
        .lock()
        .unwrap()
        .document_references(&doc_id)
        .unwrap();
    assert_eq!(refs.income_records, 1);
    assert_eq!(refs.income_checks, 1);

    let (status, _) = call(
        &app,
        Method::DELETE,
        &format!("/api/documents/{doc_id}?force=true"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    {
        let db = db_shared.lock().unwrap();
        assert_eq!(db.delete_transaction(&tx_id).unwrap(), 1);
    }
    let (_, after) = call(
        &app,
        Method::GET,
        &format!("/api/income-records/{rid}"),
        None,
    )
    .await;
    assert_eq!(after["document_ids"], json!([]));
    assert_eq!(after["transaction_ids"], json!([]));
    let (_, checks) = call(
        &app,
        Method::GET,
        "/api/income-checks?profile_id=alex",
        None,
    )
    .await;
    assert_eq!(checks["items"][0]["document_ids"], json!([]));
}

#[tokio::test]
async fn a_profile_with_income_sources_cannot_be_deleted() {
    let (app, _db) = test_router();
    make_source(&app, "alex", "employment", "Employer A").await;
    let (status, body) = call(&app, Method::DELETE, "/api/profiles/alex", None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "profile_in_use");
}

// ── Migration ────────────────────────────────────────────────────────────────

fn employment_pay_exists(path: &std::path::Path) -> bool {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.prepare("SELECT id FROM employment_pay LIMIT 0")
        .is_ok()
}

#[test]
fn migration_drops_an_empty_employment_pay_table_and_keeps_a_used_one() {
    let dir = tempdir().unwrap().keep();
    for (rows, kept) in [(0, false), (1, true)] {
        let path = dir.join(format!("migrate_{rows}.db"));
        drop(Db::open(&path).unwrap());
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch("CREATE TABLE employment_pay (id TEXT PRIMARY KEY)")
                .unwrap();
            if rows > 0 {
                conn.execute("INSERT INTO employment_pay (id) VALUES ('x')", [])
                    .unwrap();
            }
        }
        drop(Db::open(&path).unwrap());
        assert_eq!(employment_pay_exists(&path), kept, "rows = {rows}");
    }
}

#[tokio::test]
async fn unknown_enum_values_and_missing_documents_are_400s() {
    let (app, _db) = test_router();
    let id = make_source(&app, "alex", "employment", "Employer A").await;

    // A basis outside period / year_to_date fails deserialisation: 400, not 422.
    let (status, body) = post_record(
        &app,
        payslip(
            &id,
            "2025-04-01",
            "2025-04-30",
            "2025-04-28",
            json!([{ "line_type": "taxable_pay", "amount": "1", "basis": "weekly" }]),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "invalid_body");

    let (status, body) = call(
        &app,
        Method::POST,
        "/api/income-sources",
        Some(json!({ "profile_id": "alex", "source_type": "lottery", "name": "X" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "invalid_body");

    // Document ids must exist on records and checks.
    let (status, body) = post_record(
        &app,
        json!({ "source_id": id, "period_start": "2025-04-01", "period_end": "2025-04-30",
                "pay_date": "2025-04-28", "document_ids": ["no-such-doc"],
                "line_items": [{ "line_type": "taxable_pay", "amount": "1" }] }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "document_not_found");
    let (status, body) = call(
        &app,
        Method::POST,
        "/api/income-checks",
        Some(
            json!({ "source_id": id, "tax_year": "2025-26", "check_type": "p60",
                     "figures": [], "document_ids": ["no-such-doc"] }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "document_not_found");
}

#[tokio::test]
async fn employment_months_follow_the_pay_date() {
    let (app, _db) = test_router();
    let id = make_source(&app, "alex", "employment", "Employer A").await;
    // Period ends in April but was paid in March: it counts as March.
    let (status, body) = post_record(
        &app,
        payslip(
            &id,
            "2025-03-06",
            "2025-04-05",
            "2025-03-28",
            json!([{ "line_type": "taxable_pay", "amount": "100" }]),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["tax_year"], "2024-25");
    let s = summary(&app, "alex", "2024-25").await;
    assert_eq!(s["employment"][0]["months_covered"], json!(["2025-03"]));
}

#[tokio::test]
async fn bulk_and_account_deletes_strip_transactions_from_income_records() {
    let (app, db_shared) = test_router();
    let ids = {
        let db = db_shared.lock().unwrap();
        make_account(&db, "acct", "Current", &["alex"]);
        let salary = leaf(&db, "Xt Pay", "Xt Salary", CategoryType::IncomeTaxable);
        db.insert_transactions_bulk(
            "acct",
            &[
                txn("2025-04-28T09:00:00", "Net pay", dec!(660), &salary.id),
                txn("2025-05-28T09:00:00", "Net pay", dec!(670), &salary.id),
            ],
        )
        .unwrap();
        let (rows, _) = db.get_transactions(&TransactionFilters::default()).unwrap();
        rows.iter().map(|r| r.id.clone()).collect::<Vec<_>>()
    };
    let source = make_source(&app, "alex", "employment", "Employer A").await;
    let mut rids = Vec::new();
    for (tid, start, end) in [
        (&ids[0], "2025-04-01", "2025-04-30"),
        (&ids[1], "2025-05-01", "2025-05-31"),
    ] {
        let (status, rec) = post_record(
            &app,
            json!({ "source_id": source, "period_start": start, "period_end": end,
                    "transaction_ids": [tid],
                    "line_items": [{ "line_type": "net_pay", "amount": "660" }] }),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{rec}");
        rids.push(rec["id"].as_str().unwrap().to_string());
    }
    {
        let db = db_shared.lock().unwrap();
        assert_eq!(db.delete_transactions(&ids[..1]).unwrap(), 1);
        assert_eq!(db.delete_transactions_for_account("acct").unwrap(), 1);
    }
    for rid in rids {
        let (_, rec) = call(
            &app,
            Method::GET,
            &format!("/api/income-records/{rid}"),
            None,
        )
        .await;
        assert_eq!(rec["transaction_ids"], json!([]));
    }
}
