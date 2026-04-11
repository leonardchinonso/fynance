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
    pub category: Option<String>,  // Set manually or by a future categorizer
    pub tags: Vec<String>,
    pub memo: Option<String>,
    pub source: SourceFormat,
    pub fingerprint: String,       // SHA-256 hash for dedup
}

// Only CSV is supported for now. Additional formats added in later phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceFormat { Csv }

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
            tags: vec![],
            memo: None,
            source,
            fingerprint,
        }
    }
}
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

pub fn parse_date(s: &str) -> Option<chrono::NaiveDate> {
    // Try common bank date formats
    for fmt in &["%m/%d/%Y", "%Y-%m-%d", "%m-%d-%Y"] {
        if let Ok(d) = chrono::NaiveDate::parse_from_str(s, fmt) {
            return Some(d);
        }
    }
    None
}
```

Add to `Cargo.toml`:
```toml
once_cell = "1"
sha2 = "0.10"
hex = "0.4"
```

## SQLite Schema (`sql/schema.sql`)

Only the tables needed for import. Budgets, review queue, and accounts are deferred.

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
    tags        TEXT NOT NULL DEFAULT '[]',
    memo        TEXT,
    source      TEXT NOT NULL,        -- csv (only value for now)
    fingerprint TEXT UNIQUE,
    imported_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_date     ON transactions(date);
CREATE INDEX IF NOT EXISTS idx_category ON transactions(category);
CREATE INDEX IF NOT EXISTS idx_account  ON transactions(account);

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

## Useful SQL Queries

```sql
-- All transactions for a month
SELECT date, description, amount, account, category
FROM transactions
WHERE date >= '2026-04-01' AND date <= '2026-04-30'
ORDER BY date DESC;

-- Total spending by account
SELECT account, COUNT(*) as count,
       ROUND(SUM(CAST(amount AS REAL)), 2) as net
FROM transactions
GROUP BY account
ORDER BY net ASC;

-- Date range and row count
SELECT COUNT(*) as total, MIN(date) as earliest, MAX(date) as latest
FROM transactions;
```
