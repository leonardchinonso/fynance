# Data Storage and Obsidian Integration

## Recommended Architecture: SQLite + Dataview

The best approach uses SQLite as the primary data store with Obsidian markdown notes for dashboards and reports. Rust accesses SQLite through `rusqlite`.

### Why SQLite

- Handles 100,000+ transactions with sub-millisecond queries
- Single file, easy to back up and sync (iCloud, Dropbox, Obsidian Sync)
- SQL queries for arbitrary aggregations
- Works with the SQLite DB Obsidian plugin for inline visualization
- `rusqlite` with the `bundled` feature embeds SQLite at compile time: no system dependency

### Why Not Pure Markdown

- Markdown tables with 10,000+ rows become slow and unwieldy
- Dataview's index gets sluggish on large vaults with many tables
- Hard to do cross-note aggregations at scale

## SQLite Schema

```sql
CREATE TABLE IF NOT EXISTS transactions (
    id          TEXT PRIMARY KEY,
    date        TEXT NOT NULL,             -- ISO 8601: YYYY-MM-DD
    post_date   TEXT,
    description TEXT NOT NULL,             -- Normalized merchant name
    raw_desc    TEXT NOT NULL,             -- Original unmodified description
    amount      TEXT NOT NULL,             -- Decimal stored as string for precision
    account     TEXT NOT NULL,
    bank        TEXT NOT NULL,
    category    TEXT,
    confidence  REAL,
    tags        TEXT NOT NULL DEFAULT '[]',
    memo        TEXT,
    source      TEXT NOT NULL,             -- csv | ofx | qfx | pdf
    fitid       TEXT,
    fingerprint TEXT,
    imported_at TEXT NOT NULL,
    UNIQUE(fitid) ON CONFLICT IGNORE,
    UNIQUE(fingerprint) ON CONFLICT IGNORE
);

CREATE INDEX IF NOT EXISTS idx_date       ON transactions(date);
CREATE INDEX IF NOT EXISTS idx_category   ON transactions(category);
CREATE INDEX IF NOT EXISTS idx_account    ON transactions(account);
```

### Money Storage Decision

Store `amount` as TEXT rather than REAL to avoid floating-point rounding. Rust's `rust_decimal::Decimal` serializes to a plain string and back with no loss. Aggregations use `CAST(amount AS REAL)` for SQL math, which is acceptable for display but the source of truth stays as the string representation.

Alternative: store as INTEGER cents. Works great for SQL aggregations (no CAST needed) but loses the relationship to the original displayed value. Going with TEXT keeps the raw bank amount faithful.

## Rust Storage Layer

```rust
use rusqlite::{params, Connection, OptionalExtension};
use rust_decimal::Decimal;
use std::path::Path;
use std::str::FromStr;

pub struct Db {
    pub conn: Connection,
}

impl Db {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(include_str!("../../sql/schema.sql"))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        Ok(Self { conn })
    }

    pub fn insert_transaction(&self, txn: &crate::model::Transaction) -> anyhow::Result<InsertResult> {
        let res = self.conn.execute(
            r#"INSERT INTO transactions
               (id, date, post_date, description, raw_desc, amount, account, bank,
                source, fitid, fingerprint, imported_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, datetime('now'))"#,
            params![
                txn.id,
                txn.date.to_string(),
                txn.post_date.map(|d| d.to_string()),
                txn.description,
                txn.raw_description,
                txn.amount.to_string(),
                txn.account,
                txn.bank,
                format!("{:?}", txn.source).to_lowercase(),
                txn.fitid,
                txn.fingerprint,
            ],
        );
        match res {
            Ok(_) => Ok(InsertResult::Inserted),
            Err(rusqlite::Error::SqliteFailure(e, _))
                if e.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                Ok(InsertResult::Duplicate)
            }
            Err(e) => Err(e.into()),
        }
    }
}

pub enum InsertResult { Inserted, Duplicate }
```

## Vault Structure

```
~/SecondBrain/
└── financial/
    ├── transactions.db          <- SQLite (source of truth)
    ├── raw-exports/             <- Original bank files (never modified)
    │   ├── chase-checking-2024.csv
    │   └── apple-card-2025.csv
    ├── dashboard.md             <- Main overview with live queries
    ├── monthly/
    │   ├── 2026-04.md
    │   └── 2026-03.md
    └── yearly/
        └── 2025.md
```

## Obsidian Plugin Stack

| Plugin | Purpose |
|---|---|
| **Dataview** | Query markdown frontmatter and inline data |
| **SQLite DB** | Run SQL queries and render charts from `transactions.db` |
| **Templater** | Auto-generate monthly report notes |
| **Charts** | Visualize spending trends |

## Dashboard Example

See `plans/05_obsidian_integration.md` for full template. A sample query block:

````markdown
## This Month at a Glance

```sqlitedb chart:pie
SELECT category as label, ROUND(CAST(amount AS REAL) * -1, 2) as value
FROM transactions
WHERE date >= date('now', 'start of month')
  AND CAST(amount AS REAL) < 0
  AND category NOT LIKE 'Finance: Internal%'
GROUP BY category
ORDER BY value DESC
LIMIT 8
```
````

## Backup and Sync

- `transactions.db` lives inside the Obsidian vault, which is already synced via iCloud (or Obsidian Sync)
- `raw-exports/` can also be synced for audit trail
- WAL files (`-wal`, `-shm`) should be ignored by sync to avoid corruption; use `PRAGMA wal_checkpoint(TRUNCATE)` before closing if syncing the DB file directly
