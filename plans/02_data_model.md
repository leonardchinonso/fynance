# Data Model

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
    pub post_date: Option<NaiveDate>,
    pub description: String,       // Normalized merchant name
    pub raw_description: String,   // Original unmodified bank string
    pub amount: Decimal,           // Negative = debit, positive = credit
    pub account: String,           // e.g. "chase-checking"
    pub bank: String,
    pub category: Option<String>,
    pub confidence: Option<f64>,
    pub tags: Vec<String>,
    pub memo: Option<String>,
    pub source: SourceFormat,
    pub fitid: Option<String>,     // OFX unique ID for dedup
    pub fingerprint: String,       // SHA-256 hash for CSV/PDF dedup
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceFormat { Csv, Ofx, Qfx, Pdf }

impl Transaction {
    pub fn new(
        date: NaiveDate,
        description: String,
        raw_description: String,
        amount: Decimal,
        account: String,
        bank: String,
        source: SourceFormat,
    ) -> Self {
        let fingerprint = crate::util::fingerprint(
            &date.to_string(), &amount, &raw_description
        );
        Self {
            id: Uuid::new_v4().to_string()[..16].to_string(),
            date,
            post_date: None,
            description,
            raw_description,
            amount,
            account,
            bank,
            category: None,
            confidence: None,
            tags: vec![],
            memo: None,
            source,
            fitid: None,
            fingerprint,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetEntry {
    pub year_month: String,    // YYYY-MM
    pub category: String,
    pub amount: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub bank: String,
    pub name: String,
    pub account_type: AccountType,
    pub last_import: Option<NaiveDate>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccountType { Checking, Savings, Credit }
```

## Description Normalization (`src/util.rs`)

```rust
use regex::Regex;
use once_cell::sync::Lazy;
use sha2::{Digest, Sha256};

static NORMALIZE_RULES: Lazy<Vec<(Regex, &'static str)>> = Lazy::new(|| vec![
    (Regex::new(r"\s*#\d{3,}").unwrap(), ""),           // strip store numbers
    (Regex::new(r"\s+[A-Z]{2}\s*$").unwrap(), ""),      // strip trailing state codes
    (Regex::new(r"\s+[A-Z0-9]{8,}$").unwrap(), ""),     // strip trailing transaction IDs
    (Regex::new(r"\s+").unwrap(), " "),                  // normalize whitespace
    (Regex::new(r"[.,;:]+$").unwrap(), ""),              // trim trailing punctuation
]);

pub fn normalize_description(raw: &str) -> String {
    let mut s = raw.trim().to_uppercase();
    for (re, replacement) in NORMALIZE_RULES.iter() {
        s = re.replace_all(&s, *replacement).into_owned();
    }
    s.trim().to_string()
}

pub fn fingerprint(date: &str, amount: &rust_decimal::Decimal, description: &str) -> String {
    let key = format!("{}|{:.2}|{}", date, amount,
        description.chars().take(50).collect::<String>());
    let hash = Sha256::digest(key.as_bytes());
    hex::encode(&hash[..8])
}
```

Add to `Cargo.toml`:
```toml
once_cell = "1"
sha2 = "0.10"
hex = "0.4"
```

## SQLite Schema (`sql/schema.sql`)

```sql
CREATE TABLE IF NOT EXISTS transactions (
    id          TEXT PRIMARY KEY,
    date        TEXT NOT NULL,        -- YYYY-MM-DD
    post_date   TEXT,
    description TEXT NOT NULL,        -- Normalized
    raw_desc    TEXT NOT NULL,        -- Original
    amount      TEXT NOT NULL,        -- Decimal as string
    account     TEXT NOT NULL,
    bank        TEXT NOT NULL,
    category    TEXT,
    confidence  REAL,
    tags        TEXT NOT NULL DEFAULT '[]',
    memo        TEXT,
    source      TEXT NOT NULL,        -- csv|ofx|qfx|pdf
    fitid       TEXT,
    fingerprint TEXT,
    imported_at TEXT NOT NULL,
    UNIQUE(fitid),
    UNIQUE(fingerprint)
);

CREATE INDEX IF NOT EXISTS idx_date       ON transactions(date);
CREATE INDEX IF NOT EXISTS idx_category   ON transactions(category);
CREATE INDEX IF NOT EXISTS idx_account    ON transactions(account);

CREATE TABLE IF NOT EXISTS budgets (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    year_month  TEXT NOT NULL,        -- YYYY-MM
    category    TEXT NOT NULL,
    amount      TEXT NOT NULL,        -- Decimal as string
    UNIQUE(year_month, category) ON CONFLICT REPLACE
);

CREATE TABLE IF NOT EXISTS accounts (
    id          TEXT PRIMARY KEY,
    bank        TEXT NOT NULL,
    name        TEXT NOT NULL,
    type        TEXT NOT NULL,
    last_import TEXT,
    created_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS review_queue (
    transaction_id  TEXT PRIMARY KEY REFERENCES transactions(id),
    suggested       TEXT,
    confidence      REAL,
    created_at      TEXT NOT NULL,
    reviewed        INTEGER NOT NULL DEFAULT 0,
    final_category  TEXT
);

CREATE TABLE IF NOT EXISTS import_log (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    filename    TEXT NOT NULL,
    account     TEXT NOT NULL,
    imported_at TEXT NOT NULL,
    inserted    INTEGER NOT NULL DEFAULT 0,
    duplicates  INTEGER NOT NULL DEFAULT 0,
    errors      INTEGER NOT NULL DEFAULT 0
);
```

## Category Taxonomy (`config/categories.yaml`)

```yaml
categories:
  income:
    - "Income: Salary"
    - "Income: Refund"
    - "Income: Transfer In"

  fixed:
    - "Housing: Rent/Mortgage"
    - "Housing: Utilities"
    - "Housing: Insurance"
    - "Finance: Loan Payment"

  variable_needs:
    - "Food: Groceries"
    - "Transport: Gas"
    - "Transport: Rideshare & Transit"
    - "Transport: Parking & Tolls"
    - "Health: Medical & Dental"
    - "Health: Pharmacy"
    - "Health: Fitness"

  discretionary:
    - "Food: Dining & Bars"
    - "Food: Coffee"
    - "Digital: Subscriptions"
    - "Digital: Apps & Software"
    - "Shopping: Clothing"
    - "Shopping: Electronics"
    - "Shopping: Amazon & Online"
    - "Life: Entertainment"
    - "Life: Travel"
    - "Life: Personal Care"

  financial:
    - "Finance: Internal Transfer"
    - "Finance: Fees & Interest"
    - "Finance: Investment"

  other:
    - "Other"
```

## Useful SQL Queries

```sql
-- Monthly spending by category
SELECT category, ROUND(SUM(CAST(amount AS REAL)) * -1, 2) as spent
FROM transactions
WHERE date >= '2026-04-01' AND date <= '2026-04-30'
  AND CAST(amount AS REAL) < 0
  AND category NOT LIKE 'Finance: Internal%'
GROUP BY category
ORDER BY spent DESC;

-- Net savings by month (last 12 months)
SELECT
    strftime('%Y-%m', date) as month,
    ROUND(SUM(CASE WHEN CAST(amount AS REAL) > 0 THEN CAST(amount AS REAL) ELSE 0 END), 2) as income,
    ROUND(SUM(CASE WHEN CAST(amount AS REAL) < 0 THEN CAST(amount AS REAL) ELSE 0 END) * -1, 2) as expenses,
    ROUND(SUM(CAST(amount AS REAL)), 2) as net
FROM transactions
WHERE category NOT LIKE 'Finance: Internal%'
  AND date >= date('now', '-12 months')
GROUP BY month
ORDER BY month DESC;

-- Budget vs actual (current month)
SELECT
    b.category,
    CAST(b.amount AS REAL) as budget,
    COALESCE(ROUND(SUM(CAST(t.amount AS REAL)) * -1, 2), 0) as spent,
    ROUND(CAST(b.amount AS REAL) - COALESCE(SUM(CAST(t.amount AS REAL)) * -1, 0), 2) as remaining
FROM budgets b
LEFT JOIN transactions t
    ON t.category = b.category
    AND strftime('%Y-%m', t.date) = b.year_month
    AND CAST(t.amount AS REAL) < 0
WHERE b.year_month = strftime('%Y-%m', 'now')
GROUP BY b.category
ORDER BY remaining ASC;

-- Items needing review
SELECT t.date, t.description, t.amount, r.suggested, r.confidence
FROM review_queue r
JOIN transactions t ON t.id = r.transaction_id
WHERE r.reviewed = 0
ORDER BY r.confidence ASC
LIMIT 20;
```
