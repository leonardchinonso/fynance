# Data Model

## SQLite Schema

```sql
-- ── transactions ──────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS transactions (
    id              TEXT PRIMARY KEY,          -- UUID v4
    date            TEXT NOT NULL,             -- ISO 8601: YYYY-MM-DD
    description     TEXT NOT NULL,             -- raw merchant string from bank
    normalized      TEXT NOT NULL,             -- cleaned for display
    amount          TEXT NOT NULL,             -- Decimal as string, negative = debit
    currency        TEXT NOT NULL DEFAULT 'GBP',
    account_id      TEXT NOT NULL,
    category        TEXT,                      -- NULL = uncategorized
    category_source TEXT,                      -- 'rule' | 'claude' | 'manual'
    confidence      REAL,                      -- 0.0..1.0, NULL for rule/manual
    notes           TEXT,                      -- user annotation
    fingerprint     TEXT NOT NULL UNIQUE,      -- SHA-256 for dedup
    fitid           TEXT,                      -- OFX FITID if present
    created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_tx_date        ON transactions(date);
CREATE INDEX IF NOT EXISTS idx_tx_account     ON transactions(account_id);
CREATE INDEX IF NOT EXISTS idx_tx_category    ON transactions(category);
CREATE INDEX IF NOT EXISTS idx_tx_month       ON transactions(substr(date, 1, 7));

-- ── import_log ────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS import_log (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    filename        TEXT NOT NULL,
    account_id      TEXT NOT NULL,
    rows_total      INTEGER NOT NULL,
    rows_inserted   INTEGER NOT NULL,
    rows_duplicate  INTEGER NOT NULL,
    imported_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

-- ── accounts ──────────────────────────────────────────────────────────────
-- Accounts are registered manually or auto-created on first import.
-- Balance is a point-in-time snapshot, updated when user provides a statement.
CREATE TABLE IF NOT EXISTS accounts (
    id              TEXT PRIMARY KEY,          -- e.g. 'monzo-current'
    name            TEXT NOT NULL,             -- display name
    institution     TEXT NOT NULL,             -- 'Monzo', 'Revolut', 'Lloyds', etc.
    type            TEXT NOT NULL,             -- 'checking' | 'savings' | 'investment' | 'credit' | 'cash' | 'pension'
    currency        TEXT NOT NULL DEFAULT 'GBP',
    balance         TEXT,                      -- Decimal string, latest known balance
    balance_date    TEXT,                      -- date of last balance update
    is_active       INTEGER NOT NULL DEFAULT 1,
    notes           TEXT
);

-- ── portfolio_snapshots ───────────────────────────────────────────────────
-- Historical net worth snapshots for trend charts.
CREATE TABLE IF NOT EXISTS portfolio_snapshots (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    snapshot_date   TEXT NOT NULL,             -- YYYY-MM-DD
    account_id      TEXT NOT NULL,
    balance         TEXT NOT NULL,             -- Decimal string
    currency        TEXT NOT NULL DEFAULT 'GBP',
    UNIQUE(snapshot_date, account_id)
);

CREATE INDEX IF NOT EXISTS idx_snap_date ON portfolio_snapshots(snapshot_date);

-- ── budgets ───────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS budgets (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    month           TEXT NOT NULL,             -- YYYY-MM
    category        TEXT NOT NULL,
    amount          TEXT NOT NULL,             -- Decimal string, monthly budget cap
    UNIQUE(month, category)
);

CREATE INDEX IF NOT EXISTS idx_budget_month ON budgets(month);

-- ── monthly_income ────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS monthly_income (
    month           TEXT PRIMARY KEY,          -- YYYY-MM
    amount          TEXT NOT NULL,             -- Decimal string, expected income
    notes           TEXT
);

-- ── category_rules ────────────────────────────────────────────────────────
-- Persisted cache of loaded YAML rules (for UI display/editing later).
-- Primary source of truth remains config/rules.yaml.
CREATE TABLE IF NOT EXISTS category_rules (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    pattern         TEXT NOT NULL,
    category        TEXT NOT NULL,
    priority        INTEGER NOT NULL DEFAULT 0,
    source          TEXT NOT NULL DEFAULT 'yaml'  -- 'yaml' | 'user'
);
```

## Rust Types

```rust
// src/model.rs

use rust_decimal::Decimal;
use chrono::NaiveDate;

#[derive(Debug, Clone)]
pub struct Transaction {
    pub id: String,
    pub date: NaiveDate,
    pub description: String,
    pub normalized: String,
    pub amount: Decimal,        // negative = money out
    pub currency: String,
    pub account_id: String,
    pub category: Option<String>,
    pub category_source: Option<CategorySource>,
    pub confidence: Option<f64>,
    pub notes: Option<String>,
    pub fingerprint: String,
    pub fitid: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CategorySource {
    Rule,
    Claude,
    Manual,
}

#[derive(Debug, Clone)]
pub struct Account {
    pub id: String,
    pub name: String,
    pub institution: String,
    pub account_type: AccountType,
    pub currency: String,
    pub balance: Option<Decimal>,
    pub balance_date: Option<NaiveDate>,
    pub is_active: bool,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AccountType {
    Checking,
    Savings,
    Investment,
    Credit,
    Cash,
    Pension,
}

#[derive(Debug, Clone)]
pub struct Budget {
    pub month: String,          // YYYY-MM
    pub category: String,
    pub amount: Decimal,
}

#[derive(Debug, Clone)]
pub struct PortfolioSnapshot {
    pub snapshot_date: NaiveDate,
    pub account_id: String,
    pub balance: Decimal,
    pub currency: String,
}
```

## Category Taxonomy

Categories use a two-level hierarchy: `Parent: Child`.

```
Income
  ├── Salary
  ├── Freelance
  ├── Investments
  └── Other Income

Housing
  ├── Rent / Mortgage
  ├── Utilities
  ├── Internet & Phone
  └── Home Maintenance

Food
  ├── Groceries
  ├── Dining & Bars
  └── Coffee & Cafes

Transport
  ├── Public Transit
  ├── Taxi & Rideshare
  ├── Fuel
  └── Car Maintenance

Health
  ├── Gym & Fitness
  ├── Medical & Dental
  └── Pharmacy

Shopping
  ├── Clothing
  ├── Electronics
  └── General

Entertainment
  ├── Streaming Services
  ├── Events & Concerts
  └── Hobbies

Travel
  ├── Flights
  ├── Accommodation
  └── Holiday Spending

Finance
  ├── Savings Transfer
  ├── Investment Transfer
  ├── Fees & Charges
  └── Insurance

Personal Care
  └── Haircut & Beauty

Gifts & Donations
  └── Gifts

Education
  └── Courses & Books

Other
  └── Uncategorized
```

## Key Design Decisions

1. **Decimal as TEXT in SQLite**: SQLite has no native decimal type. Storing as TEXT (e.g. `"-42.50"`) with `rust_decimal::Decimal` parsing avoids floating-point errors. Queries that need numeric comparison cast with `CAST(amount AS REAL)` — acceptable since these are comparisons, not summations that accumulate error.

2. **Accounts table for portfolio**: Rather than inferring balances from transaction sums (which requires complete history), the `accounts` table holds point-in-time balance snapshots. Users update balances after each import. The `portfolio_snapshots` table records these over time for trend charts.

3. **Fingerprint deduplication**: Every transaction gets a SHA-256 fingerprint of `(date, amount, description, account_id)`. This catches duplicates across re-imports even without an OFX FITID.

4. **Category rules in YAML, not just DB**: `config/rules.yaml` is the source of truth for rules so they can be version-controlled. The DB table is a read cache for the UI.
