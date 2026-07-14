//! Integration tests for `storage::db`: bulk category reassignment, the
//! step-14 account/holding type collapse in `migrate_schema`, and the
//! investment-history series behind the "Cumulative invested" chart.

use chrono::{NaiveDate, NaiveDateTime};
use fynance::model::{
    Account, AccountType, CategorySource, CategoryType, CreateCategoryPayload,
    CreateInvestmentEventBody, Currency, Granularity, Holding, HoldingType, InvestmentHistoryRow,
    Transaction,
};
use fynance::storage::Db;
use fynance::util::fx::FxRateMap;
use rusqlite::{Connection, params};
use rust_decimal::Decimal;
use std::path::PathBuf;
use std::str::FromStr;
use tempfile::tempdir;

fn temp_db_path(name: &str) -> PathBuf {
    let dir = tempdir().unwrap();
    dir.keep().join(name)
}

fn test_db(name: &str) -> Db {
    Db::open(&temp_db_path(name)).unwrap()
}

fn dec(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap()
}

fn date(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

fn datetime(s: &str) -> NaiveDateTime {
    date(s).and_hms_opt(0, 0, 0).unwrap()
}

fn account(id: &str, account_type: AccountType) -> Account {
    Account {
        id: id.to_string(),
        name: id.to_string(),
        institution: "TestBank".to_string(),
        account_type,
        currency: "GBP".to_string(),
        balance: None,
        balance_date: None,
        is_active: true,
        notes: None,
        profile_ids: vec!["default".to_string()],
        is_stale: None,
        is_available: true,
    }
}

// ── 1. bulk_update_transaction_category ──────────────────────────────────────

struct Cats {
    parent: String,
    leaf_a: String,
    leaf_b: String,
    inactive_leaf: String,
}

/// Parent + two active leaves + one soft-deleted leaf, all named out of the way
/// of the seeded default taxonomy (categories.name is UNIQUE).
fn seed_test_categories(db: &Db) -> Cats {
    let make = |name: &str, parent_id: Option<&str>| {
        db.create_category(&CreateCategoryPayload {
            name: name.to_string(),
            parent_id: parent_id.map(str::to_string),
            display_order: Some(900),
            description: None,
            category_type: CategoryType::Spending,
        })
        .unwrap()
        .id
    };

    let parent = make("ZZ Test Parent", None);
    let leaf_a = make("ZZ Test Leaf A", Some(&parent));
    let leaf_b = make("ZZ Test Leaf B", Some(&parent));
    let inactive_leaf = make("ZZ Test Leaf Retired", Some(&parent));
    db.soft_delete_category(&inactive_leaf).unwrap();

    Cats {
        parent,
        leaf_a,
        leaf_b,
        inactive_leaf,
    }
}

fn seed_transactions(db: &Db, category_id: &str, ids: &[&str]) {
    db.create_account(&account("monzo", AccountType::Checking))
        .unwrap();
    for id in ids {
        db.insert_transaction(&Transaction {
            id: (*id).to_string(),
            date: datetime("2026-01-05"),
            description: format!("Test {id}"),
            normalized: format!("test {id}"),
            amount: dec("-12.50"),
            currency: "GBP".to_string(),
            account_id: "monzo".to_string(),
            category_id: Some(category_id.to_string()),
            category_source: Some(CategorySource::Rule),
            confidence: None,
            notes: None,
            is_recurring: false,
            exclude_from_summary: false,
            fingerprint: format!("fp-{id}"),
            fitid: None,
            source_document_ids: Vec::new(),
        })
        .unwrap();
    }
}

fn category_of(db: &Db, tx_id: &str) -> Option<String> {
    db.get_transaction_by_id(tx_id)
        .unwrap()
        .unwrap()
        .category_id
}

fn source_of(db: &Db, tx_id: &str) -> Option<CategorySource> {
    db.get_transaction_by_id(tx_id)
        .unwrap()
        .unwrap()
        .category_source
}

#[test]
fn bulk_update_category_reassigns_only_the_listed_transactions() {
    let db = test_db("bulk_happy.db");
    let cats = seed_test_categories(&db);
    seed_transactions(&db, &cats.leaf_a, &["t1", "t2", "t3", "t4"]);

    let ids = vec!["t1".to_string(), "t2".to_string(), "t3".to_string()];
    let changed = db
        .bulk_update_transaction_category(&ids, &cats.leaf_b, CategorySource::Manual)
        .unwrap();

    assert_eq!(changed, 3);
    for id in ["t1", "t2", "t3"] {
        assert_eq!(category_of(&db, id).as_deref(), Some(cats.leaf_b.as_str()));
        assert_eq!(source_of(&db, id), Some(CategorySource::Manual));
    }
    assert_eq!(
        category_of(&db, "t4").as_deref(),
        Some(cats.leaf_a.as_str()),
        "a transaction outside the id list must be untouched"
    );
    assert_eq!(source_of(&db, "t4"), Some(CategorySource::Rule));
}

#[test]
fn bulk_update_category_rejects_a_parent_category() {
    let db = test_db("bulk_parent.db");
    let cats = seed_test_categories(&db);
    seed_transactions(&db, &cats.leaf_a, &["t1", "t2"]);

    let ids = vec!["t1".to_string(), "t2".to_string()];
    let err = db
        .bulk_update_transaction_category(&ids, &cats.parent, CategorySource::Manual)
        .unwrap_err();
    assert!(err.to_string().contains("parent"), "got: {err}");

    for id in ["t1", "t2"] {
        assert_eq!(
            category_of(&db, id).as_deref(),
            Some(cats.leaf_a.as_str()),
            "a rejected bulk update must not mutate any row"
        );
    }
}

#[test]
fn bulk_update_category_rejects_an_inactive_category() {
    let db = test_db("bulk_inactive.db");
    let cats = seed_test_categories(&db);
    seed_transactions(&db, &cats.leaf_a, &["t1", "t2"]);

    let ids = vec!["t1".to_string(), "t2".to_string()];
    let err = db
        .bulk_update_transaction_category(&ids, &cats.inactive_leaf, CategorySource::Manual)
        .unwrap_err();
    assert!(err.to_string().contains("inactive"), "got: {err}");

    for id in ["t1", "t2"] {
        assert_eq!(
            category_of(&db, id).as_deref(),
            Some(cats.leaf_a.as_str()),
            "a rejected bulk update must not mutate any row"
        );
    }
}

#[test]
fn bulk_update_category_with_no_ids_is_a_noop() {
    let db = test_db("bulk_empty.db");
    let cats = seed_test_categories(&db);
    seed_transactions(&db, &cats.leaf_a, &["t1"]);

    let changed = db
        .bulk_update_transaction_category(&[], &cats.leaf_b, CategorySource::Manual)
        .unwrap();
    assert_eq!(changed, 0);

    // The empty list short-circuits ahead of category validation, so even an
    // unresolvable category id is not an error.
    let changed = db
        .bulk_update_transaction_category(&[], "no-such-category", CategorySource::Manual)
        .unwrap();
    assert_eq!(changed, 0);

    assert_eq!(
        category_of(&db, "t1").as_deref(),
        Some(cats.leaf_a.as_str())
    );
}

// ── 2. migrate_schema step 14: collapse removed type variants ────────────────

fn insert_legacy_account(conn: &Connection, id: &str, account_type: &str) {
    conn.execute(
        r#"INSERT INTO accounts (id, name, institution, type, currency, is_active, notes, profile_ids)
           VALUES (?1, ?1, 'TestBank', ?2, 'GBP', 1, NULL, '["default"]')"#,
        params![id, account_type],
    )
    .unwrap();
}

fn insert_legacy_holding(
    conn: &Connection,
    account_id: &str,
    symbol: &str,
    holding_type: &str,
    value: &str,
) {
    conn.execute(
        r"INSERT INTO holdings (account_id, symbol, name, holding_type, quantity, value, currency, as_of)
          VALUES (?1, ?2, ?2, ?3, '1', ?4, 'GBP', '2026-01-31T00:00:00')",
        params![account_id, symbol, holding_type, value],
    )
    .unwrap();
}

fn scalar_i64(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get(0)).unwrap()
}

fn account_type_of(conn: &Connection, id: &str) -> String {
    conn.query_row(
        "SELECT type FROM accounts WHERE id = ?1",
        params![id],
        |r| r.get(0),
    )
    .unwrap()
}

fn holding_type_of(conn: &Connection, symbol: &str) -> String {
    conn.query_row(
        "SELECT holding_type FROM holdings WHERE symbol = ?1",
        params![symbol],
        |r| r.get(0),
    )
    .unwrap()
}

/// Sum of every holding's value, read straight from SQLite as TEXT and parsed
/// as `Decimal`. This is the quantity the migration must never move.
fn holdings_value_sum(conn: &Connection) -> Decimal {
    let mut stmt = conn.prepare("SELECT value FROM holdings").unwrap();
    let values: Vec<String> = stmt
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    values.iter().map(|v| dec(v)).sum()
}

fn assert_migration_14_applied(conn: &Connection, expected_sum: Decimal, expected_rows: i64) {
    assert_eq!(
        scalar_i64(conn, "SELECT COUNT(*) FROM accounts WHERE type = 'cash'"),
        0,
        "no 'cash' account type may survive the migration"
    );
    assert_eq!(account_type_of(conn, "legacy_cash"), "checking");
    assert_eq!(
        account_type_of(conn, "card"),
        "credit",
        "'credit' is still a valid ACCOUNT type; only the holding type collapses"
    );

    assert_eq!(
        scalar_i64(
            conn,
            "SELECT COUNT(*) FROM holdings WHERE holding_type IN ('savings', 'loan', 'credit')"
        ),
        0,
        "no removed holding type may survive the migration"
    );
    assert_eq!(holding_type_of(conn, "_CASH"), "cash");
    assert_eq!(holding_type_of(conn, "MORTGAGE"), "debt");
    assert_eq!(holding_type_of(conn, "CARD_BALANCE"), "debt");
    assert_eq!(holding_type_of(conn, "AAPL"), "stock");

    assert_eq!(
        scalar_i64(conn, "SELECT COUNT(*) FROM holdings"),
        expected_rows,
        "the migration retypes rows, it never adds or drops them"
    );
    assert_eq!(
        holdings_value_sum(conn),
        expected_sum,
        "balances must be preserved exactly: the migration only retypes"
    );
}

#[test]
fn migration_14_collapses_legacy_types_and_preserves_balances() {
    let path = temp_db_path("migration14.db");
    // First open builds the schema; the pre-migration rows go in behind its back
    // (raw SQL) because the typed API can no longer express the removed variants.
    drop(Db::open(&path).unwrap());

    let (before_sum, before_rows) = {
        let conn = Connection::open(&path).unwrap();
        insert_legacy_account(&conn, "legacy_cash", "cash");
        insert_legacy_account(&conn, "brokerage", "investment");
        insert_legacy_account(&conn, "house", "property");
        insert_legacy_account(&conn, "card", "credit");

        insert_legacy_holding(&conn, "legacy_cash", "_CASH", "savings", "1500.25");
        insert_legacy_holding(&conn, "brokerage", "AAPL", "stock", "5000.00");
        insert_legacy_holding(&conn, "house", "MORTGAGE", "loan", "-250000.00");
        insert_legacy_holding(&conn, "card", "CARD_BALANCE", "credit", "-430.75");

        (
            holdings_value_sum(&conn),
            scalar_i64(&conn, "SELECT COUNT(*) FROM holdings"),
        )
    };
    assert_eq!(before_sum, dec("-243930.50"));
    assert_eq!(before_rows, 4);

    drop(Db::open(&path).unwrap());
    {
        let conn = Connection::open(&path).unwrap();
        assert_migration_14_applied(&conn, before_sum, before_rows);
    }

    // Idempotent: a second startup finds no matching rows and changes nothing.
    drop(Db::open(&path).unwrap());
    {
        let conn = Connection::open(&path).unwrap();
        assert_migration_14_applied(&conn, before_sum, before_rows);
    }
}

// ── 3. get_investment_history ────────────────────────────────────────────────

fn fx_gbp() -> FxRateMap {
    FxRateMap::new(vec![Currency {
        code: "GBP".to_string(),
        is_preferred: true,
        fx_rate: Decimal::ONE,
        updated_at: None,
    }])
    .unwrap()
}

fn fx_gbp_usd(usd_rate: &str) -> FxRateMap {
    FxRateMap::new(vec![
        Currency {
            code: "GBP".to_string(),
            is_preferred: true,
            fx_rate: Decimal::ONE,
            updated_at: None,
        },
        Currency {
            code: "USD".to_string(),
            is_preferred: false,
            fx_rate: dec(usd_rate),
            updated_at: None,
        },
    ])
    .unwrap()
}

#[allow(clippy::too_many_arguments)]
fn event(
    db: &Db,
    account_id: &str,
    event_type: &str,
    symbol: &str,
    on: &str,
    quantity: &str,
    price: &str,
    fee: Option<&str>,
    currency: &str,
) {
    db.create_investment_event(&CreateInvestmentEventBody {
        account_id: account_id.to_string(),
        event_type: event_type.to_string(),
        symbol: symbol.to_string(),
        date: format!("{on}T10:00:00"),
        quantity: quantity.to_string(),
        price_per_share: price.to_string(),
        fee: fee.map(str::to_string),
        currency: currency.to_string(),
        fee_currency: None,
        notes: None,
        source_document_ids: Vec::new(),
    })
    .unwrap();
}

fn add_holding(db: &Db, account_id: &str, symbol: &str, value: &str, currency: &str, as_of: &str) {
    db.upsert_holdings(
        account_id,
        &[Holding {
            account_id: account_id.to_string(),
            symbol: symbol.to_string(),
            name: symbol.to_string(),
            holding_type: HoldingType::Stock,
            quantity: Decimal::ONE,
            price_per_unit: None,
            value: dec(value),
            currency: currency.to_string(),
            as_of: datetime(as_of),
            short_name: None,
            sub_account: None,
            is_closed: false,
            derived: false,
            source_document_ids: Vec::new(),
            source_file: None,
        }],
    )
    .unwrap();
}

fn history(
    db: &Db,
    from: &str,
    to: &str,
    account_ids: &[String],
    fx: &FxRateMap,
) -> Vec<InvestmentHistoryRow> {
    db.get_investment_history(
        date(from),
        date(to),
        &Granularity::Monthly,
        None,
        account_ids,
        fx,
    )
    .unwrap()
}

fn net(row: &InvestmentHistoryRow) -> Option<Decimal> {
    row.net_invested.as_ref().map(|s| dec(s))
}

fn market(row: &InvestmentHistoryRow) -> Option<Decimal> {
    row.market_value.as_ref().map(|s| dec(s))
}

/// The crux of the metric: a disposal removes the shares' average BOOK COST
/// from net invested, never the sale PROCEEDS. Sell 40 of 100 shares bought at
/// 10 for 25 each and the proceeds (1,000) equal the entire original outlay:
/// proceeds-based logic would report 0 invested. The correct answer is 600.
#[test]
fn investment_history_sell_removes_book_cost_not_proceeds() {
    let db = test_db("inv_hist_book_cost.db");
    db.create_account(&account("gia", AccountType::Investment))
        .unwrap();
    event(
        &db,
        "gia",
        "buy",
        "AAPL",
        "2026-01-15",
        "100",
        "10",
        None,
        "GBP",
    );
    event(
        &db,
        "gia",
        "sell",
        "AAPL",
        "2026-03-10",
        "40",
        "25",
        None,
        "GBP",
    );

    let rows = history(&db, "2026-01-01", "2026-03-31", &[], &fx_gbp());
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].period, "2026-01");
    assert_eq!(rows[2].period, "2026-03");

    assert_eq!(net(&rows[0]), Some(dec("1000")));
    assert_eq!(net(&rows[1]), Some(dec("1000")));
    assert_eq!(
        net(&rows[2]),
        Some(dec("600")),
        "40 shares at an average book cost of 10 removes 400, not the 1,000 of proceeds"
    );
}

/// `withhold` is a disposal too (shares taken for tax at vest), and it uses the
/// same average book cost: withholding at a price well above cost must not
/// remove more than was ever contributed.
#[test]
fn investment_history_withhold_removes_book_cost() {
    let db = test_db("inv_hist_withhold.db");
    db.create_account(&account("gia", AccountType::Investment))
        .unwrap();
    event(
        &db,
        "gia",
        "vest",
        "RSU",
        "2026-01-10",
        "100",
        "10",
        None,
        "GBP",
    );
    event(
        &db,
        "gia",
        "withhold",
        "RSU",
        "2026-01-20",
        "30",
        "25",
        None,
        "GBP",
    );

    let rows = history(&db, "2026-01-01", "2026-01-31", &[], &fx_gbp());
    assert_eq!(rows.len(), 1);
    assert_eq!(net(&rows[0]), Some(dec("700")));
}

/// A fee on an acquisition is capital contributed, so it lifts net invested and
/// the per-symbol pool cost (which a later disposal then draws down at the
/// fee-inclusive average).
#[test]
fn investment_history_acquisition_fee_increases_net_invested_and_pool_cost() {
    let db = test_db("inv_hist_fee.db");
    db.create_account(&account("gia", AccountType::Investment))
        .unwrap();
    event(
        &db,
        "gia",
        "buy",
        "VWRL",
        "2026-01-15",
        "10",
        "100",
        Some("25"),
        "GBP",
    );
    event(
        &db,
        "gia",
        "sell",
        "VWRL",
        "2026-02-10",
        "5",
        "200",
        None,
        "GBP",
    );

    let rows = history(&db, "2026-01-01", "2026-02-28", &[], &fx_gbp());
    assert_eq!(net(&rows[0]), Some(dec("1025")));
    assert_eq!(
        net(&rows[1]),
        Some(dec("512.50")),
        "half the pool leaves at the fee-inclusive average cost of 102.50"
    );
}

/// Foreign-currency events are converted to the preferred currency, price and
/// fee alike, and so is the market value of the holdings.
#[test]
fn investment_history_converts_foreign_currency_amounts() {
    let db = test_db("inv_hist_fx.db");
    db.create_account(&account("gia", AccountType::Investment))
        .unwrap();
    event(
        &db,
        "gia",
        "buy",
        "AAPL",
        "2026-01-15",
        "10",
        "100",
        Some("20"),
        "USD",
    );
    event(
        &db,
        "gia",
        "buy",
        "VWRL",
        "2026-02-10",
        "1",
        "100",
        None,
        "GBP",
    );
    add_holding(&db, "gia", "AAPL", "1000", "USD", "2026-01-31");

    let rows = history(&db, "2026-01-01", "2026-02-28", &[], &fx_gbp_usd("0.74"));

    assert_eq!(
        net(&rows[0]),
        Some(dec("754.80")),
        "(10 x 100 + 20) USD at 0.74 = 754.80 GBP"
    );
    assert_eq!(
        net(&rows[1]),
        Some(dec("854.80")),
        "the GBP buy is added unconverted on top"
    );
    assert_eq!(market(&rows[0]), Some(dec("740")));
}

/// `account_ids` scopes both halves of the row, not just the events.
#[test]
fn investment_history_account_ids_filter_scopes_events_and_market_value() {
    let db = test_db("inv_hist_accounts.db");
    db.create_account(&account("gia_a", AccountType::Investment))
        .unwrap();
    db.create_account(&account("gia_b", AccountType::Investment))
        .unwrap();
    event(
        &db,
        "gia_a",
        "buy",
        "AAPL",
        "2026-01-10",
        "10",
        "100",
        None,
        "GBP",
    );
    event(
        &db,
        "gia_b",
        "buy",
        "MSFT",
        "2026-01-12",
        "5",
        "100",
        None,
        "GBP",
    );
    add_holding(&db, "gia_a", "AAPL", "1200", "GBP", "2026-01-31");
    add_holding(&db, "gia_b", "MSFT", "600", "GBP", "2026-01-31");

    let only_a = history(
        &db,
        "2026-01-01",
        "2026-01-31",
        &["gia_a".to_string()],
        &fx_gbp(),
    );
    assert_eq!(net(&only_a[0]), Some(dec("1000")));
    assert_eq!(market(&only_a[0]), Some(dec("1200")));

    let all = history(&db, "2026-01-01", "2026-01-31", &[], &fx_gbp());
    assert_eq!(net(&all[0]), Some(dec("1500")));
    assert_eq!(market(&all[0]), Some(dec("1800")));
}

/// A period with no data yet is a gap (`None`), not a phantom zero: the chart
/// must not draw a flat line back to the start of the range.
#[test]
fn investment_history_periods_before_the_first_event_are_gaps() {
    let db = test_db("inv_hist_gap.db");
    db.create_account(&account("gia", AccountType::Investment))
        .unwrap();
    event(
        &db,
        "gia",
        "buy",
        "AAPL",
        "2026-03-05",
        "10",
        "100",
        None,
        "GBP",
    );
    add_holding(&db, "gia", "AAPL", "1100", "GBP", "2026-03-31");

    let rows = history(&db, "2026-01-01", "2026-03-31", &[], &fx_gbp());
    assert_eq!(rows.len(), 3);

    assert_eq!(
        rows[0].net_invested, None,
        "January precedes the first event: expected a gap, not Some(\"0\")"
    );
    assert_eq!(rows[1].net_invested, None);
    assert_eq!(rows[0].market_value, None);
    assert_eq!(rows[1].market_value, None);

    assert_eq!(net(&rows[2]), Some(dec("1000")));
    assert_eq!(market(&rows[2]), Some(dec("1100")));
}

/// Only `investment` and `investment_isa` accounts feed the series. Pension and
/// everyday accounts are out of scope on both the event and the holding side.
#[test]
fn investment_history_counts_only_investment_and_isa_accounts() {
    let db = test_db("inv_hist_types.db");
    db.create_account(&account("gia", AccountType::Investment))
        .unwrap();
    db.create_account(&account("isa", AccountType::InvestmentIsa))
        .unwrap();
    db.create_account(&account("sipp", AccountType::Pension))
        .unwrap();
    db.create_account(&account("monzo", AccountType::Checking))
        .unwrap();

    event(
        &db,
        "gia",
        "buy",
        "AAPL",
        "2026-01-10",
        "10",
        "100",
        None,
        "GBP",
    );
    event(
        &db,
        "isa",
        "buy",
        "VWRL",
        "2026-01-11",
        "5",
        "100",
        None,
        "GBP",
    );
    event(
        &db,
        "sipp",
        "buy",
        "BOND",
        "2026-01-12",
        "20",
        "100",
        None,
        "GBP",
    );
    event(
        &db,
        "monzo",
        "buy",
        "GOLD",
        "2026-01-13",
        "3",
        "100",
        None,
        "GBP",
    );

    add_holding(&db, "gia", "AAPL", "1200", "GBP", "2026-01-31");
    add_holding(&db, "isa", "VWRL", "800", "GBP", "2026-01-31");
    add_holding(&db, "sipp", "BOND", "9999", "GBP", "2026-01-31");
    add_holding(&db, "monzo", "GOLD", "500", "GBP", "2026-01-31");

    let rows = history(&db, "2026-01-01", "2026-01-31", &[], &fx_gbp());
    assert_eq!(
        net(&rows[0]),
        Some(dec("1500")),
        "only the GIA (1,000) and ISA (500) contributions count"
    );
    assert_eq!(
        market(&rows[0]),
        Some(dec("2000")),
        "only the GIA (1,200) and ISA (800) holdings count"
    );
}

/// `created_at` is written with a trailing `Z` (by both the SQL column default and
/// the Rust insert), but the row mapper falls back to `now()` when it cannot parse
/// a stored datetime. If the parser rejects the `Z`, every read silently returns
/// the current time instead of the real one, and the fallback hides the failure.
#[test]
fn created_at_is_read_from_the_row_not_regenerated() {
    let db = test_db("created_at_round_trip.db");
    db.create_account(&account("gia", AccountType::Investment))
        .unwrap();

    db.create_investment_event(&CreateInvestmentEventBody {
        account_id: "gia".to_string(),
        event_type: "buy".to_string(),
        symbol: "AAPL".to_string(),
        date: "2026-01-05T00:00:00".to_string(),
        quantity: "10".to_string(),
        price_per_share: "150.00".to_string(),
        fee: None,
        currency: "GBP".to_string(),
        fee_currency: None,
        notes: None,
        source_document_ids: Vec::new(),
    })
    .unwrap();

    let first = db.list_investment_events(None, None, None, None).unwrap();
    let created = first[0].created_at;

    // A fabricated now() differs on a later read; a value actually read from the
    // row cannot. Sleep past the column's one-second resolution.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let second = db.list_investment_events(None, None, None, None).unwrap();

    assert_eq!(
        created, second[0].created_at,
        "created_at changed between reads, so it is being regenerated rather than read from the row"
    );
}
