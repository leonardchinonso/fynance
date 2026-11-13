//! `GET /api/transactions/by-category` must return per-category SUMS.
//!
//! A refactor dropped the aggregate from the grouped SQL, leaving bare
//! expressions; SQLite then answers such queries with one arbitrary row per
//! group instead of the total. These tests pin the sum semantics.
//! All data synthetic.

use chrono::NaiveDate;
use fynance::model::{Account, AccountType, CreateCategoryPayload, ImportTransaction};
use fynance::model::{CategoryTotal, TransactionDirection};
use fynance::storage::{Db, TransactionFilters};
use fynance::util::fx::FxRateMap;
use rust_decimal::Decimal;
use std::str::FromStr;
use tempfile::NamedTempFile;

macro_rules! dec {
    ($val:expr) => {
        Decimal::from_str(stringify!($val)).unwrap()
    };
}

fn gbp_usd_fx() -> FxRateMap {
    FxRateMap::new(vec![
        fynance::model::Currency {
            code: "GBP".to_string(),
            is_preferred: true,
            fx_rate: dec!(1),
            updated_at: None,
        },
        fynance::model::Currency {
            code: "USD".to_string(),
            is_preferred: false,
            fx_rate: dec!(0.5),
            updated_at: None,
        },
    ])
    .unwrap()
}

fn make_account(db: &Db, id: &str) {
    db.create_account(&Account {
        id: id.to_string(),
        name: id.to_string(),
        institution: "TestBank".to_string(),
        account_type: AccountType::Checking,
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
    category_type: fynance::model::CategoryType,
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
    y: i32,
    m: u32,
    d: u32,
    description: &str,
    amount: Decimal,
    currency: &str,
    category_id: Option<&str>,
) -> ImportTransaction {
    ImportTransaction {
        date: NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap(),
        description: description.to_string(),
        amount,
        currency: Some(currency.to_string()),
        category_id: category_id.map(str::to_string),
        category_source: None,
        notes: None,
        is_recurring: None,
        exclude_from_summary: None,
        source_document_ids: Vec::new(),
    }
}

fn filters() -> TransactionFilters {
    TransactionFilters {
        start: Some(NaiveDate::from_ymd_opt(2025, 1, 1).unwrap()),
        end: Some(NaiveDate::from_ymd_opt(2025, 1, 31).unwrap()),
        ..TransactionFilters::default()
    }
}

fn total_for<'a>(rows: &'a [CategoryTotal], category_id: &str) -> &'a str {
    rows.iter()
        .find(|r| r.category_id.as_deref() == Some(category_id))
        .expect("category present in response")
        .total
        .as_str()
}

fn seeded_db() -> (Db, NamedTempFile, String, String) {
    let file = NamedTempFile::new().expect("temp file");
    let db = Db::open(file.path()).expect("test db");
    make_account(&db, "chk");
    let food = leaf(
        &db,
        "T-Group Food",
        "T Food",
        fynance::model::CategoryType::Spending,
    );
    let other = leaf(
        &db,
        "T-Group Home",
        "T Home",
        fynance::model::CategoryType::Spending,
    );
    db.insert_transactions_bulk(
        "chk",
        &[
            txn(2025, 1, 5, "big shop", dec!(-400), "GBP", Some(&food.id)),
            txn(2025, 1, 6, "refund", dec!(50), "GBP", Some(&food.id)),
            txn(2025, 1, 7, "lunch", dec!(-150), "GBP", Some(&food.id)),
            txn(2025, 1, 8, "takeaway", dec!(-200.50), "GBP", Some(&food.id)),
            // A second currency in the same category: each currency leg must
            // be summed separately and converted, never picked row-by-row.
            txn(2025, 1, 9, "usd dinner", dec!(-100), "USD", Some(&food.id)),
            txn(2025, 1, 10, "usd refund", dec!(20), "USD", Some(&food.id)),
            txn(2025, 1, 12, "chair", dec!(-90), "GBP", Some(&other.id)),
        ],
    )
    .unwrap();
    (db, file, food.id, other.id)
}

#[test]
fn outflow_totals_sum_every_row_not_one_row_per_category() {
    let (db, _f, food, _other) = seeded_db();
    let fx = gbp_usd_fx();
    let rows = db
        .get_transactions_by_category(&filters(), Some(TransactionDirection::Outflow), &fx)
        .unwrap();
    // GBP legs: 400 + 150 + 200.50 = 750.50; USD legs: 100 x 0.5 = 50.
    assert_eq!(total_for(&rows, &food), "800.5");
}

#[test]
fn income_totals_sum_positive_legs_per_category() {
    let (db, _f, food, _other) = seeded_db();
    // A second GBP refund in the same category: with only one income row
    // per (category, currency) group, SQLite's bare-column behaviour would
    // coincidentally match the sum, so this extra row is what pins SUM here.
    db.insert_transactions_bulk(
        "chk",
        &[txn(
            2025,
            1,
            14,
            "second refund",
            dec!(25),
            "GBP",
            Some(food.as_str()),
        )],
    )
    .unwrap();
    let fx = gbp_usd_fx();
    let rows = db
        .get_transactions_by_category(&filters(), Some(TransactionDirection::Income), &fx)
        .unwrap();
    // Refunds: 50 + 25 GBP + 20 USD x 0.5 = 85 (rendered "85.0" through the
    // f64 leg of the existing totals pipeline).
    assert_eq!(total_for(&rows, &food), "85.0");
}

#[test]
fn undirected_totals_are_signed_net_sums_per_category() {
    let (db, _f, food, _other) = seeded_db();
    let fx = gbp_usd_fx();
    let rows = db
        .get_transactions_by_category(&filters(), None, &fx)
        .unwrap();
    // Net of every leg: (-400+50-150-200.50) + (-100+20) x 0.5 = -740.5.
    assert_eq!(total_for(&rows, &food), "-740.5");
}
