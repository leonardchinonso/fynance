//! Regression tests for the portfolio overview's headline figures.
//! All data synthetic.

use chrono::NaiveDate;
use fynance::model::{
    Account, AccountType, CreateCategoryPayload, CreateInvestmentEventBody, ExchangeRate, Holding,
    HoldingType, ImportTransaction,
};
use fynance::storage::Db;
use rust_decimal::Decimal;
use std::str::FromStr;
use tempfile::NamedTempFile;

macro_rules! dec {
    ($val:expr) => {
        Decimal::from_str(stringify!($val)).unwrap()
    };
}

fn test_db() -> (Db, NamedTempFile) {
    let file = NamedTempFile::new().expect("temp file");
    let db = Db::open(file.path()).expect("test db");
    (db, file)
}

fn dt(y: i32, m: u32, d: u32) -> chrono::NaiveDateTime {
    NaiveDate::from_ymd_opt(y, m, d)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap()
}

fn gbp_fx() -> fynance::util::fx::FxRateMap {
    fynance::util::fx::FxRateMap::new(vec![fynance::model::Currency {
        code: "GBP".to_string(),
        is_preferred: true,
        fx_rate: dec!(1),
        updated_at: None,
    }])
    .unwrap()
}

fn make_account(id: &str, account_type: AccountType) -> Account {
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

fn holding(
    account_id: &str,
    symbol: &str,
    value: Decimal,
    as_of: chrono::NaiveDateTime,
) -> Holding {
    Holding {
        account_id: account_id.to_string(),
        symbol: symbol.to_string(),
        name: symbol.to_string(),
        holding_type: HoldingType::Stock,
        quantity: Decimal::ONE,
        price_per_unit: None,
        value,
        currency: "GBP".to_string(),
        as_of,
        short_name: None,
        sub_account: None,
        is_closed: false,
        derived: false,
        source_document_ids: Vec::new(),
        source_file: None,
    }
}

fn event(
    account_id: &str,
    event_type: &str,
    symbol: &str,
    date: &str,
    quantity: &str,
    price_per_share: &str,
    currency: &str,
) -> CreateInvestmentEventBody {
    CreateInvestmentEventBody {
        account_id: account_id.to_string(),
        event_type: event_type.to_string(),
        symbol: symbol.to_string(),
        date: date.to_string(),
        quantity: quantity.to_string(),
        price_per_share: price_per_share.to_string(),
        fee: None,
        currency: currency.to_string(),
        fee_currency: None,
        notes: None,
        source_document_ids: Vec::new(),
    }
}

fn cat(name: &str, category_type: fynance::model::CategoryType) -> CreateCategoryPayload {
    CreateCategoryPayload {
        name: name.to_string(),
        parent_id: None,
        display_order: None,
        description: None,
        category_type,
    }
}

fn txn(
    date: chrono::NaiveDateTime,
    description: &str,
    amount: Decimal,
    category_id: &str,
) -> ImportTransaction {
    ImportTransaction {
        date,
        description: description.to_string(),
        amount,
        currency: None,
        category_id: Some(category_id.to_string()),
        category_source: None,
        notes: None,
        is_recurring: None,
        exclude_from_summary: None,
        source_document_ids: Vec::new(),
    }
}

fn rate(base: &str, date: &str, rate: Decimal) -> ExchangeRate {
    ExchangeRate {
        base: base.to_string(),
        quote: "GBP".to_string(),
        date: date.to_string(),
        rate,
        source: "user".to_string(),
        updated_at: None,
    }
}

// ── Problem 1: spending is outflows only; income is inflows only ────────────

fn leaf(
    db: &Db,
    parent_name: &str,
    name: &str,
    category_type: fynance::model::CategoryType,
) -> fynance::model::Category {
    let parent = db
        .create_category(&cat(parent_name, category_type))
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

#[test]
fn spending_counts_outflows_only_income_counts_inflows_only() {
    let (db, _f) = test_db();
    db.create_account(&make_account("chk", AccountType::Checking))
        .unwrap();
    let food = leaf(
        &db,
        "T-Group Food",
        "T Food",
        fynance::model::CategoryType::Spending,
    );
    let salary = leaf(
        &db,
        "T-Group Pay",
        "Pay",
        fynance::model::CategoryType::IncomeTaxable,
    );

    db.insert_transactions_bulk(
        "chk",
        &[
            txn(dt(2025, 1, 5), "shop", dec!(-400), &food.id),
            txn(dt(2025, 1, 6), "refund", dec!(50), &food.id),
            txn(dt(2025, 1, 10), "pay", dec!(1000), &salary.id),
            txn(dt(2025, 1, 11), "pay reversal", dec!(-100), &salary.id),
        ],
    )
    .unwrap();

    let (income, spending) = db
        .compute_category_type_cash(
            dt(2025, 1, 1).date(),
            dt(2025, 1, 31).date(),
            None,
            &gbp_fx(),
        )
        .unwrap();
    assert_eq!(
        income,
        dec!(1000),
        "income = inflows only, not netted with the reversal"
    );
    assert_eq!(
        spending,
        dec!(400),
        "spending = outflows only, not netted with the refund"
    );
}

// Pins the type-exclusion invariant (internal_transfer / interest belong to
// neither bucket) against future refactors of the bucket sets. The signed-
// netting regression itself is caught by the test above.
#[test]
fn internal_transfer_and_interest_types_excluded_from_both() {
    let (db, _f) = test_db();
    db.create_account(&make_account("chk", AccountType::Checking))
        .unwrap();
    db.create_account(&make_account("sav", AccountType::Savings))
        .unwrap();
    let transfer = leaf(
        &db,
        "T-Group Moving",
        "Transfers",
        fynance::model::CategoryType::InternalTransfer,
    );
    let interest = leaf(
        &db,
        "T-Group Earn",
        "Interest",
        fynance::model::CategoryType::InterestNonTaxable,
    );
    let bills = leaf(
        &db,
        "T-Group Home",
        "T Bills",
        fynance::model::CategoryType::Spending,
    );

    db.insert_transactions_bulk(
        "chk",
        &[
            txn(dt(2025, 1, 5), "to savings", dec!(-1000), &transfer.id),
            txn(dt(2025, 1, 5), "card repayment", dec!(-2000), &transfer.id),
            txn(dt(2025, 1, 7), "bank interest", dec!(30), &interest.id),
            txn(dt(2025, 1, 8), "energy", dec!(-250), &bills.id),
        ],
    )
    .unwrap();
    db.insert_transactions_bulk(
        "sav",
        &[txn(
            dt(2025, 1, 5),
            "from checking",
            dec!(1000),
            &transfer.id,
        )],
    )
    .unwrap();

    let (income, spending) = db
        .compute_category_type_cash(
            dt(2025, 1, 1).date(),
            dt(2025, 1, 31).date(),
            None,
            &gbp_fx(),
        )
        .unwrap();
    assert_eq!(
        income,
        dec!(0),
        "interest inflow is not income-taxable income"
    );
    assert_eq!(
        spending,
        dec!(250),
        "own-account transfers are not spending"
    );
}

// ── Problem 2: growth metrics and cash cover one account set ────────────────

#[test]
fn investment_metrics_and_new_cash_cover_the_investments_asset_class() {
    let (db, _f) = test_db();
    db.create_account(&make_account("inv", AccountType::Investment))
        .unwrap();
    db.create_account(&make_account("isa", AccountType::InvestmentIsa))
        .unwrap();
    db.create_account(&make_account("pen", AccountType::Pension))
        .unwrap();
    db.create_account(&make_account("chk", AccountType::Checking))
        .unwrap();

    // Both in-scope accounts are valued on 31 Dec, before the 1 Jan period
    // start, so they open the period; a valuation dated 1 Jan itself would
    // be period activity (pinned by the day-1 test below).
    db.upsert_holdings(
        "inv",
        &[
            holding("inv", "AAPL", dec!(100), dt(2024, 12, 31)),
            holding("inv", "AAPL", dec!(150), dt(2025, 6, 1)),
        ],
    )
    .unwrap();
    // ISA: 100 entering the period; a 20 buy on the first day shows up in
    // the same-day 120 valuation.
    db.upsert_holdings(
        "isa",
        &[
            holding("isa", "VWRP", dec!(100), dt(2024, 12, 31)),
            holding("isa", "VWRP", dec!(120), dt(2025, 1, 1)),
        ],
    )
    .unwrap();
    db.upsert_holdings(
        "pen",
        &[holding("pen", "FUND", dec!(9000), dt(2024, 12, 31))],
    )
    .unwrap();
    db.upsert_holdings(
        "chk",
        &[holding("chk", "_CASH", dec!(777), dt(2024, 12, 31))],
    )
    .unwrap();

    for body in [
        event(
            "inv",
            "buy",
            "AAPL",
            "2025-02-01T00:00:00",
            "1",
            "50",
            "GBP",
        ),
        event(
            "isa",
            "buy",
            "VWRP",
            "2025-01-01T00:00:00",
            "1",
            "20",
            "GBP",
        ),
        event(
            "pen",
            "buy",
            "FUND",
            "2025-02-01T00:00:00",
            "1",
            "5000",
            "GBP",
        ),
        event(
            "chk",
            "buy",
            "BTC",
            "2025-02-01T00:00:00",
            "1",
            "999",
            "GBP",
        ),
    ] {
        db.create_investment_event(&body).unwrap();
    }

    let m = db
        .compute_investment_metrics(
            dt(2025, 1, 1).date(),
            dt(2025, 6, 30).date(),
            None,
            &gbp_fx(),
        )
        .unwrap();
    // Value covers investment + ISA only, opening at the value held entering
    // the period (a first-day valuation is activity, not the opening).
    assert_eq!(m.start_value, dec!(200));
    assert_eq!(m.end_value, dec!(270));
    assert_eq!(m.total_growth, dec!(70));
    // And the cash covers exactly those accounts: pension and checking buys
    // must not leak in, or market_growth absorbs them as "performance".
    // The day-1 ISA buy counts once, as new cash, not as a phantom loss.
    assert_eq!(m.new_cash_invested, dec!(70));
    assert_eq!(m.market_growth, dec!(0));
}

#[test]
fn transfers_between_in_scope_accounts_net_to_zero_and_out_of_scope_transfers_are_ignored() {
    let (db, _f) = test_db();
    db.create_account(&make_account("a", AccountType::Investment))
        .unwrap();
    db.create_account(&make_account("b", AccountType::Investment))
        .unwrap();
    db.create_account(&make_account("ext", AccountType::Checking))
        .unwrap();

    for body in [
        event(
            "a",
            "transfer",
            "VWRP",
            "2025-02-01T00:00:00",
            "-1",
            "100",
            "GBP",
        ),
        event(
            "b",
            "transfer",
            "VWRP",
            "2025-02-01T00:00:00",
            "1",
            "100",
            "GBP",
        ),
        event(
            "ext",
            "transfer",
            "VWRP",
            "2025-02-01T00:00:00",
            "-1",
            "40",
            "GBP",
        ),
    ] {
        db.create_investment_event(&body).unwrap();
    }

    let total = db
        .compute_new_cash_invested(
            dt(2025, 1, 1).date(),
            dt(2025, 6, 30).date(),
            None,
            &gbp_fx(),
        )
        .unwrap();
    assert_eq!(
        total,
        dec!(0),
        "a transfer between two in-scope accounts nets to zero"
    );
}

#[test]
fn new_cash_invested_converts_at_event_date_rate() {
    let (db, _f) = test_db();
    db.create_account(&make_account("us", AccountType::Investment))
        .unwrap();
    db.upsert_exchange_rates(&[
        rate("USD", "2025-01-15", dec!(0.75)),
        rate("USD", "2025-06-15", dec!(0.74)),
    ])
    .unwrap();

    db.create_investment_event(&event(
        "us",
        "buy",
        "AAPL",
        "2025-02-01T00:00:00",
        "10",
        "100",
        "USD",
    ))
    .unwrap();

    // Current flat rate 0.90 must NOT be used; the most recent stored rate
    // before the event (0.75 on 2025-01-15) is.
    let fx = fynance::util::fx::FxRateMap::new(vec![
        fynance::model::Currency {
            code: "GBP".to_string(),
            is_preferred: true,
            fx_rate: dec!(1),
            updated_at: None,
        },
        fynance::model::Currency {
            code: "USD".to_string(),
            is_preferred: false,
            fx_rate: dec!(0.9),
            updated_at: None,
        },
    ])
    .unwrap()
    .with_historical(db.get_exchange_rates_for_quote("GBP").unwrap());

    let total = db
        .compute_new_cash_invested(dt(2025, 1, 1).date(), dt(2025, 6, 30).date(), None, &fx)
        .unwrap();
    assert_eq!(
        total,
        dec!(750),
        "1000 USD at the 0.75 rate stored nearest the event date"
    );
}

// ── Problem 3: period change measured from the period's first day ───────────

#[test]
fn period_opening_is_the_first_day_not_the_first_bucket_end() {
    let (db, _f) = test_db();
    db.create_account(&make_account("inv", AccountType::Investment))
        .unwrap();
    db.upsert_holdings(
        "inv",
        &[
            holding("inv", "AAPL", dec!(500), dt(2025, 1, 5)),
            holding("inv", "AAPL", dec!(800), dt(2025, 2, 5)),
        ],
    )
    .unwrap();

    let fx = gbp_fx();
    // Opening value = nothing entering the period's first day; the first
    // bucket (Jan 31) already includes the Jan 5 snapshot.
    assert_eq!(
        db.net_worth_entering(dt(2025, 1, 1).date(), None, &fx)
            .unwrap(),
        dec!(0)
    );
    assert_eq!(
        db.net_worth_entering(dt(2025, 1, 10).date(), None, &fx)
            .unwrap(),
        dec!(500)
    );

    let history = db
        .get_monthly_net_worth(
            dt(2025, 1, 1).date(),
            dt(2025, 2, 28).date(),
            &fynance::model::Granularity::Monthly,
            None,
            &fx,
        )
        .unwrap();
    assert_eq!(history[0].total_wealth, dec!(500));
    assert_eq!(history[1].total_wealth, dec!(800));
    // The delta from the period's first day is 800, not the 300 you get by
    // measuring from the first bucket's month-end.
    let opening = db
        .net_worth_entering(dt(2025, 1, 1).date(), None, &fx)
        .unwrap();
    assert_eq!(history[1].total_wealth - opening, dec!(800));
}

#[test]
fn first_day_valuation_is_period_activity_not_the_opening() {
    let (db, _f) = test_db();
    db.create_account(&make_account("inv", AccountType::Investment))
        .unwrap();
    // A valuation dated the period's first day (date-only rows store
    // T00:00:00) belongs to the change over the period, and the opening
    // excludes it exactly as the per-account cards' carry-forward does.
    db.upsert_holdings(
        "inv",
        &[
            holding("inv", "AAPL", dec!(250), dt(2025, 1, 1)),
            holding("inv", "AAPL", dec!(500), dt(2025, 1, 15)),
        ],
    )
    .unwrap();

    let fx = gbp_fx();
    assert_eq!(
        db.net_worth_entering(dt(2025, 1, 1).date(), None, &fx)
            .unwrap(),
        dec!(0)
    );
    assert_eq!(
        db.net_worth_entering(dt(2025, 1, 2).date(), None, &fx)
            .unwrap(),
        dec!(250)
    );

    let history = db
        .get_monthly_net_worth(
            dt(2025, 1, 1).date(),
            dt(2025, 1, 31).date(),
            &fynance::model::Granularity::Monthly,
            None,
            &fx,
        )
        .unwrap();
    assert_eq!(history[0].total_wealth, dec!(500));
    // The day-1 valuation counts in the period change, not the opening.
    let opening = db
        .net_worth_entering(dt(2025, 1, 1).date(), None, &fx)
        .unwrap();
    assert_eq!(history[0].total_wealth - opening, dec!(500));
}

// ── Problem 4: Net Savings = income - spending ──────────────────────────────

// Drives `GET /api/budget/cash-summary` in-process (the way server_smoke
// does) so the test pins the response contract itself, not just the
// arithmetic: the field is named net_savings, and it is income minus
// spending with transfers and interest in neither term.
#[tokio::test]
async fn cash_summary_net_savings_is_income_minus_spending_ignoring_transfers_and_interest() {
    use axum::body::{Body, to_bytes};
    use axum::http::{Method, Request, StatusCode};
    use fynance::server::build_router;
    use std::sync::{Arc, Mutex};
    use tower::ServiceExt;

    let (db, _f) = test_db();
    db.create_account(&make_account("chk", AccountType::Checking))
        .unwrap();
    db.create_account(&make_account("sav", AccountType::Savings))
        .unwrap();
    let salary = leaf(
        &db,
        "T-Group Pay",
        "Pay",
        fynance::model::CategoryType::IncomeTaxable,
    );
    let bills = leaf(
        &db,
        "T-Group Home",
        "T Bills",
        fynance::model::CategoryType::Spending,
    );
    let transfer = leaf(
        &db,
        "T-Group Moving",
        "Transfers",
        fynance::model::CategoryType::InternalTransfer,
    );
    let interest = leaf(
        &db,
        "T-Group Earn",
        "Interest",
        fynance::model::CategoryType::InterestTaxable,
    );

    db.insert_transactions_bulk(
        "chk",
        &[
            txn(dt(2025, 1, 10), "pay", dec!(2000), &salary.id),
            txn(dt(2025, 1, 8), "energy", dec!(-250), &bills.id),
            txn(dt(2025, 1, 5), "to savings", dec!(-1000), &transfer.id),
            txn(dt(2025, 1, 20), "card repayment", dec!(-400), &transfer.id),
            txn(dt(2025, 1, 7), "bank interest", dec!(30), &interest.id),
        ],
    )
    .unwrap();
    db.insert_transactions_bulk(
        "sav",
        &[txn(
            dt(2025, 1, 5),
            "from checking",
            dec!(1000),
            &transfer.id,
        )],
    )
    .unwrap();

    let app = build_router(Arc::new(Mutex::new(db)), true);
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/api/budget/cash-summary?start=2025-01-01&end=2025-01-31")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // Money fields serialize as strings; reparse for exact comparison.
    let field = |name: &str| {
        Decimal::from_str(body[name].as_str().unwrap_or_else(|| {
            panic!("cash-summary response missing string field `{name}`: {body}")
        }))
        .unwrap()
    };
    let (income, spending, net_savings) =
        (field("income"), field("spending"), field("net_savings"));
    assert_eq!(
        net_savings,
        income - spending,
        "net_savings is income - spending"
    );
    assert_eq!(
        net_savings,
        dec!(1750),
        "the £1,400 of own-account transfers and the £30 of interest stay \
         out of both terms: 2000 - 250, not the £1,680 you'd get by netting \
         transfers in, the £3,400 by counting them as spending, or the £1,780 \
         by counting interest as income"
    );
}
