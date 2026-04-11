# Data Model

> **Updated after Prompt 1.1.** Schema expanded to cover accounts, portfolio snapshots, budgets, and monthly income. See `../design/03_data_model.md` for the full design and rationale.

## Core Structs (`src/model.rs`)

```rust
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub id: String,
    pub date: NaiveDate,
    pub description: String,          // Normalized merchant name
    pub raw_description: String,      // Original unmodified bank string
    pub amount: Decimal,              // Negative = debit, positive = credit
    pub currency: String,             // ISO 4217, default "GBP"
    pub account_id: String,
    pub category: Option<String>,
    pub category_source: Option<CategorySource>,
    pub confidence: Option<f64>,      // 0.0..1.0, NULL for rule/manual
    pub notes: Option<String>,
    pub fingerprint: String,          // SHA-256 hash for dedup
    pub fitid: Option<String>,        // Bank-provided ID (Monzo transaction_id, etc.)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CategorySource { Rule, Claude, Manual }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: String,                   // e.g. "monzo-current"
    pub name: String,                 // display name
    pub institution: String,          // "Monzo", "Revolut", "Lloyds"
    pub account_type: AccountType,
    pub currency: String,
    pub balance: Option<Decimal>,     // latest known point-in-time balance
    pub balance_date: Option<NaiveDate>,
    pub is_active: bool,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccountType {
    Checking,
    Savings,
    Investment,
    Credit,
    Cash,
    Pension,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Budget {
    pub month: String,                // YYYY-MM
    pub category: String,
    pub amount: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioSnapshot {
    pub snapshot_date: NaiveDate,
    pub account_id: String,
    pub balance: Decimal,
    pub currency: String,
}
```

## Description Normalization (`src/util.rs`)

```rust
use regex::Regex;
use once_cell::sync::Lazy;
use sha2::{Digest, Sha256};

static NORMALIZE_RULES: Lazy<Vec<(Regex, &'static str)>> = Lazy::new(|| vec![
    (Regex::new(r"\s*#\d{3,}").unwrap(), ""),         // strip store numbers
    (Regex::new(r"\s+[A-Z]{2}\s*$").unwrap(), ""),    // strip trailing country codes
    (Regex::new(r"\s+[A-Z0-9]{8,}$").unwrap(), ""),   // strip trailing transaction IDs
    (Regex::new(r"\s+").unwrap(), " "),               // normalize whitespace
    (Regex::new(r"[.,;:]+$").unwrap(), ""),           // trim trailing punctuation
]);

pub fn normalize_description(raw: &str) -> String {
    let mut s = raw.trim().to_uppercase();
    for (re, replacement) in NORMALIZE_RULES.iter() {
        s = re.replace_all(&s, *replacement).into_owned();
    }
    s.trim().to_string()
}

pub fn fingerprint(date: &str, amount: &rust_decimal::Decimal, description: &str, account_id: &str) -> String {
    let key = format!("{}|{:.2}|{}|{}", date, amount,
        description.chars().take(50).collect::<String>(),
        account_id);
    let hash = Sha256::digest(key.as_bytes());
    hex::encode(&hash[..16])
}

pub fn parse_date(s: &str) -> Option<chrono::NaiveDate> {
    for fmt in &["%Y-%m-%d", "%d/%m/%Y", "%Y-%m-%d %H:%M:%S", "%d-%m-%Y"] {
        if let Ok(d) = chrono::NaiveDate::parse_from_str(s, fmt) {
            return Some(d);
        }
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, fmt) {
            return Some(dt.date());
        }
    }
    None
}
```

## SQLite Schema (`sql/schema.sql`)

```sql
-- ── transactions ──────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS transactions (
    id              TEXT PRIMARY KEY,
    date            TEXT NOT NULL,              -- ISO 8601: YYYY-MM-DD
    description     TEXT NOT NULL,              -- normalized
    raw_description TEXT NOT NULL,              -- original
    amount          TEXT NOT NULL,              -- Decimal as string
    currency        TEXT NOT NULL DEFAULT 'GBP',
    account_id      TEXT NOT NULL,
    category        TEXT,
    category_source TEXT,                       -- 'rule' | 'claude' | 'manual'
    confidence      REAL,
    notes           TEXT,
    fingerprint     TEXT NOT NULL UNIQUE,
    fitid           TEXT,
    imported_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_tx_date     ON transactions(date);
CREATE INDEX IF NOT EXISTS idx_tx_account  ON transactions(account_id);
CREATE INDEX IF NOT EXISTS idx_tx_category ON transactions(category);
CREATE INDEX IF NOT EXISTS idx_tx_month    ON transactions(substr(date, 1, 7));

-- ── accounts ──────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS accounts (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL,
    institution     TEXT NOT NULL,
    type            TEXT NOT NULL,              -- checking | savings | investment | credit | cash | pension
    currency        TEXT NOT NULL DEFAULT 'GBP',
    balance         TEXT,
    balance_date    TEXT,
    is_active       INTEGER NOT NULL DEFAULT 1,
    notes           TEXT
);

-- ── portfolio_snapshots ───────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS portfolio_snapshots (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    snapshot_date   TEXT NOT NULL,
    account_id      TEXT NOT NULL,
    balance         TEXT NOT NULL,
    currency        TEXT NOT NULL DEFAULT 'GBP',
    UNIQUE(snapshot_date, account_id)
);

CREATE INDEX IF NOT EXISTS idx_snap_date ON portfolio_snapshots(snapshot_date);

-- ── budgets ───────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS budgets (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    month           TEXT NOT NULL,              -- YYYY-MM
    category        TEXT NOT NULL,
    amount          TEXT NOT NULL,
    UNIQUE(month, category) ON CONFLICT REPLACE
);

CREATE INDEX IF NOT EXISTS idx_budget_month ON budgets(month);

-- ── monthly_income ────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS monthly_income (
    month           TEXT PRIMARY KEY,           -- YYYY-MM
    amount          TEXT NOT NULL,
    notes           TEXT
);

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
```

## Useful SQL Queries

```sql
-- Spending per category for a month
SELECT
    category,
    COUNT(*) AS count,
    ROUND(SUM(ABS(CAST(amount AS REAL))), 2) AS spent
FROM transactions
WHERE substr(date, 1, 7) = ?1
  AND CAST(amount AS REAL) < 0
  AND category IS NOT NULL
  AND category != 'Finance: Internal Transfer'
GROUP BY category
ORDER BY spent DESC;

-- Net worth as of a date
SELECT
    SUM(CASE WHEN type IN ('checking', 'savings', 'investment', 'cash', 'pension')
             THEN CAST(balance AS REAL) ELSE 0 END) AS assets,
    SUM(CASE WHEN type = 'credit'
             THEN CAST(balance AS REAL) ELSE 0 END) AS liabilities
FROM accounts
WHERE is_active = 1;

-- Budget vs actual for a month
SELECT
    b.category,
    CAST(b.amount AS REAL) AS budgeted,
    COALESCE(SUM(ABS(CAST(t.amount AS REAL))), 0) AS actual,
    ROUND(CAST(b.amount AS REAL) - COALESCE(SUM(ABS(CAST(t.amount AS REAL))), 0), 2) AS remaining
FROM budgets b
LEFT JOIN transactions t
    ON t.category = b.category
    AND substr(t.date, 1, 7) = b.month
    AND CAST(t.amount AS REAL) < 0
WHERE b.month = ?1
GROUP BY b.category
ORDER BY remaining ASC;

-- Cash flow per month (12 months)
SELECT
    substr(date, 1, 7) AS month,
    ROUND(SUM(CASE WHEN CAST(amount AS REAL) > 0 THEN CAST(amount AS REAL) ELSE 0 END), 2) AS income,
    ROUND(SUM(CASE WHEN CAST(amount AS REAL) < 0 THEN ABS(CAST(amount AS REAL)) ELSE 0 END), 2) AS spending
FROM transactions
WHERE date >= date('now', '-12 months')
GROUP BY month
ORDER BY month;
```
