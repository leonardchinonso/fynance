//! Tests for the portfolio overview cash figures (exercise fixes):
//! per-leg spending, transfer-family and pairing exclusions, investment-metric
//! account scoping, and the range-opening net worth row.

use super::*;
use tempfile::NamedTempFile;

fn test_db() -> (Db, NamedTempFile) {
    let file = NamedTempFile::new().expect("temp file");
    let db = Db::open(file.path()).expect("test db");
    (db, file)
}

fn fx_map(db: &Db) -> crate::util::fx::FxRateMap {
    crate::util::fx::FxRateMap::new(db.get_currencies().unwrap()).unwrap()
}

fn mk_account(db: &Db, id: &str, account_type: AccountType) {
    let is_available = is_available_account(&account_type);
    db.create_account(&Account {
        id: id.to_string(),
        name: format!("{id} test"),
        institution: "Test Bank".to_string(),
        account_type,
        currency: "GBP".to_string(),
        balance: None,
        balance_date: None,
        is_active: true,
        notes: None,
        profile_ids: vec!["default".to_string()],
        is_stale: None,
        is_available,
    })
    .expect("account");
}

fn mk_cat(db: &Db, name: &str, parent: Option<&str>, ctype: CategoryType) -> String {
    db.create_category(&CreateCategoryPayload {
        name: name.to_string(),
        parent_id: parent.map(|s| s.to_string()),
        display_order: None,
        description: None,
        category_type: ctype,
    })
    .expect("category")
    .id
}

fn add_txn(db: &Db, id: &str, account_id: &str, date: &str, amount: &str, category: Option<&str>) {
    db.conn
        .execute(
            "INSERT INTO transactions (id, date, description, normalized, amount, currency, \
             account_id, category_id, fingerprint) VALUES (?1,?2,?3,?3,?4,'GBP',?5,?6,?7)",
            rusqlite::params![
                id,
                date,
                format!("txn {id}"),
                amount,
                account_id,
                category,
                format!("fp-{id}")
            ],
        )
        .expect("txn");
}

fn add_holding(db: &Db, account_id: &str, symbol: &str, value: &str, as_of: &str) {
    db.conn
        .execute(
            "INSERT INTO holdings (account_id, symbol, name, holding_type, quantity, value, \
             currency, as_of, source_document_ids) VALUES (?1,?2,?2,'cash','1',?3,'GBP',?4,'[]')",
            rusqlite::params![account_id, symbol, value, as_of],
        )
        .expect("holding");
}

fn d(s: &str) -> Decimal {
    s.parse().unwrap()
}

fn span() -> (NaiveDate, NaiveDate) {
    (
        NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
        NaiveDate::from_ymd_opt(2025, 6, 30).unwrap(),
    )
}

fn ymd(year: i32, month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day).unwrap()
}

#[test]
fn spending_is_per_leg_outflow_not_signed_bucket() {
    let (db, _f) = test_db();
    mk_account(&db, "acc1", AccountType::Checking);
    let groceries = mk_cat(&db, "X Groceries", None, CategoryType::Spending);
    let general = mk_cat(&db, "X General", None, CategoryType::Spending);
    let salary = mk_cat(&db, "X Salary", None, CategoryType::IncomeTaxable);

    add_txn(
        &db,
        "t1",
        "acc1",
        "2025-03-01T10:00:00",
        "-100.00",
        Some(&groceries),
    );
    add_txn(
        &db,
        "t2",
        "acc1",
        "2025-03-02T10:00:00",
        "500.00",
        Some(&general),
    );
    add_txn(
        &db,
        "t3",
        "acc1",
        "2025-03-02T11:00:00",
        "-100.00",
        Some(&general),
    );
    add_txn(
        &db,
        "t4",
        "acc1",
        "2025-03-03T10:00:00",
        "400.00",
        Some(&salary),
    );

    let (income, spending) = db
        .compute_category_type_cash(span().0, span().1, None, &fx_map(&db))
        .unwrap();
    assert_eq!(income, d("400"));
    assert_eq!(spending, d("200"));
}

#[test]
fn transfer_family_legs_are_not_spending() {
    let (db, _f) = test_db();
    mk_account(&db, "acc1", AccountType::Checking);
    let moving = mk_cat(&db, "Moving money", None, CategoryType::Spending);
    let transfers = mk_cat(&db, "X Transfers", Some(&moving), CategoryType::Spending);
    let investments = mk_cat(
        &db,
        "X Investments",
        Some(&moving),
        CategoryType::InterestTaxable,
    );
    let groceries = mk_cat(&db, "X Groceries", None, CategoryType::Spending);

    add_txn(
        &db,
        "t1",
        "acc1",
        "2025-03-01T10:00:00",
        "-900.00",
        Some(&transfers),
    );
    add_txn(
        &db,
        "t2",
        "acc1",
        "2025-03-01T11:00:00",
        "-200.00",
        Some(&investments),
    );
    add_txn(
        &db,
        "t3",
        "acc1",
        "2025-03-01T12:00:00",
        "-50.00",
        Some(&groceries),
    );

    let (_, spending) = db
        .compute_category_type_cash(span().0, span().1, None, &fx_map(&db))
        .unwrap();
    assert_eq!(spending, d("50"));
}

#[test]
fn cross_account_same_day_transfer_is_not_spending() {
    let (db, _f) = test_db();
    mk_account(&db, "acc1", AccountType::Checking);
    mk_account(&db, "acc2", AccountType::Checking);
    let shopping = mk_cat(&db, "X Shopping", None, CategoryType::Spending);

    add_txn(
        &db,
        "t1",
        "acc1",
        "2025-03-01T10:00:00",
        "-200.00",
        Some(&shopping),
    );
    add_txn(
        &db,
        "t2",
        "acc2",
        "2025-03-01T10:01:00",
        "200.00",
        Some(&shopping),
    );
    add_txn(
        &db,
        "t3",
        "acc1",
        "2025-03-01T14:00:00",
        "-60.00",
        Some(&shopping),
    );

    let (_, spending) = db
        .compute_category_type_cash(span().0, span().1, None, &fx_map(&db))
        .unwrap();
    assert_eq!(spending, d("60"));
}

#[test]
fn same_account_refund_cancels_outflow() {
    let (db, _f) = test_db();
    mk_account(&db, "acc1", AccountType::Checking);
    let cabs = mk_cat(&db, "X Cabs", None, CategoryType::Spending);

    add_txn(
        &db,
        "t1",
        "acc1",
        "2025-03-01T12:00:00",
        "-50.00",
        Some(&cabs),
    );
    add_txn(
        &db,
        "t2",
        "acc1",
        "2025-03-01T12:05:00",
        "50.00",
        Some(&cabs),
    );

    let (_, spending) = db
        .compute_category_type_cash(span().0, span().1, None, &fx_map(&db))
        .unwrap();
    assert_eq!(spending, d("0"));
}

#[test]
fn cross_currency_conversion_wire_is_not_spending() {
    let (db, _f) = test_db();
    db.create_currency("USD", d("0.75")).expect("currency");
    mk_account(&db, "acc-usd", AccountType::Checking);
    mk_account(&db, "acc-gbp", AccountType::Checking);
    let expenses = mk_cat(&db, "X Expenses", None, CategoryType::Spending);
    let transfers = mk_cat(&db, "X Transfers", None, CategoryType::Spending);

    add_txn(
        &db,
        "t1",
        "acc-usd",
        "2025-03-01T10:00:00",
        "-1000.00",
        Some(&expenses),
    );
    db.conn
        .execute("UPDATE transactions SET currency='USD' WHERE id='t1'", [])
        .unwrap();
    add_txn(
        &db,
        "t2",
        "acc-gbp",
        "2025-03-01T11:00:00",
        "750.00",
        Some(&transfers),
    );

    let (_, spending) = db
        .compute_category_type_cash(span().0, span().1, None, &fx_map(&db))
        .unwrap();
    assert_eq!(spending, d("0"));
}

#[test]
fn investment_events_are_scoped_to_investment_accounts() {
    let (db, _f) = test_db();
    mk_account(&db, "inv", AccountType::Investment);
    mk_account(&db, "isa", AccountType::InvestmentIsa);
    mk_account(&db, "pen", AccountType::Pension);

    let event = |account_id: &str, symbol: &str, date: &str, qty: &str, price: &str| {
        db.create_investment_event(&CreateInvestmentEventBody {
            account_id: account_id.to_string(),
            event_type: "buy".to_string(),
            symbol: symbol.to_string(),
            date: date.to_string(),
            quantity: qty.to_string(),
            price_per_share: price.to_string(),
            fee: None,
            currency: "GBP".to_string(),
            fee_currency: None,
            notes: None,
            source_document_ids: Vec::new(),
        })
        .expect("event");
    };
    event("inv", "VUSA", "2025-02-01T00:00:00", "10", "5");
    event("isa", "VUSA", "2025-02-02T00:00:00", "4", "25");
    event("pen", "FUND", "2025-02-03T00:00:00", "1", "500");

    let nci = db
        .compute_new_cash_invested(span().0, span().1, None, &fx_map(&db))
        .unwrap();
    assert_eq!(nci, d("150"));
}

#[test]
fn investment_metrics_cover_isa_and_pension_stays_out() {
    let (db, _f) = test_db();
    mk_account(&db, "inv", AccountType::Investment);
    mk_account(&db, "isa", AccountType::InvestmentIsa);
    mk_account(&db, "pen", AccountType::Pension);

    add_holding(&db, "inv", "idx", "40", "2024-12-31T00:00:00");
    add_holding(&db, "inv", "idx", "60", "2025-06-20T00:00:00");
    add_holding(&db, "isa", "idx", "90", "2024-12-31T00:00:00");
    add_holding(&db, "isa", "idx", "110", "2025-06-20T00:00:00");
    add_holding(&db, "pen", "idx", "400", "2024-12-31T00:00:00");
    add_holding(&db, "pen", "idx", "500", "2025-06-20T00:00:00");

    db.create_investment_event(&CreateInvestmentEventBody {
        account_id: "inv".to_string(),
        event_type: "buy".to_string(),
        symbol: "idx".to_string(),
        date: "2025-02-01T00:00:00".to_string(),
        quantity: "10".to_string(),
        price_per_share: "5".to_string(),
        fee: None,
        currency: "GBP".to_string(),
        fee_currency: None,
        notes: None,
        source_document_ids: Vec::new(),
    })
    .expect("event");

    let metrics = db
        .compute_investment_metrics(span().0, span().1, None, &fx_map(&db))
        .unwrap();
    assert_eq!(metrics.start_value, d("130"));
    assert_eq!(metrics.end_value, d("170"));
    assert_eq!(metrics.total_growth, d("40"));
    assert_eq!(metrics.new_cash_invested, d("50"));
    assert_eq!(metrics.market_growth, d("-10"));
}

#[test]
fn net_worth_row_at_carries_holdings_from_before_the_range() {
    let (db, _f) = test_db();
    mk_account(&db, "chk", AccountType::Checking);
    mk_account(&db, "prop", AccountType::Property);

    add_holding(&db, "chk", "bal", "100", "2025-01-15T00:00:00");
    add_holding(&db, "prop", "home", "500", "2024-12-01T00:00:00");

    let opening = db
        .net_worth_row_at(ymd(2025, 6, 1), None, &fx_map(&db))
        .unwrap();
    assert_eq!(opening.month, "2025-06-01");
    assert_eq!(opening.total_wealth, d("600"));
    assert_eq!(opening.available_wealth, d("100"));
    assert_eq!(opening.unavailable_wealth, d("500"));
}
