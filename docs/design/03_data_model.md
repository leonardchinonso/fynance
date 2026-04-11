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
    is_recurring    INTEGER NOT NULL DEFAULT 0, -- 1 = recurring (salary, rent, subscriptions)
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
    source          TEXT NOT NULL DEFAULT 'csv',  -- 'csv' | 'screenshot' | 'api'
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
-- Historical account-level balance snapshots for net worth trend charts.
-- One row per account per date. Used to plot "net worth over time".
-- For per-symbol detail within investment accounts, see the "holdings" table.
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
-- DISCUSS: Should budgets be standing targets (one per category) or per-month?
-- Option A (current): per-month budgets allow varying targets (e.g., higher food budget in December).
-- Option B (proposed): standing budgets are simpler; just query transactions for any month against the target.
CREATE TABLE IF NOT EXISTS budgets (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    month           TEXT NOT NULL,             -- YYYY-MM (remove if standing budgets are adopted)
    category        TEXT NOT NULL,
    amount          TEXT NOT NULL,             -- Decimal string, monthly budget cap
    UNIQUE(month, category)
);

CREATE INDEX IF NOT EXISTS idx_budget_month ON budgets(month);

-- ── holdings ──────────────────────────────────────────────────────────────
-- DISCUSS: Should holdings be a separate table or consolidated with portfolio_snapshots?
-- Nonso raised whether these overlap. Current design: "holdings" stores per-symbol
-- detail within investment accounts (e.g., VWRL: 50 units @ £160), while
-- "portfolio_snapshots" stores account-level balance snapshots for net worth trend
-- charts. Holdings drill down into an account; snapshots aggregate up to net worth.
-- Ope asked how holdings differs from portfolio. May not be needed for MVP.
-- See Open Questions in docs/fynance-project-note.md.
CREATE TABLE IF NOT EXISTS holdings (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id      TEXT NOT NULL,
    symbol          TEXT NOT NULL,             -- ticker or ISIN (e.g. 'VWRL', 'AAPL')
    name            TEXT NOT NULL,             -- display name (e.g. 'Vanguard FTSE All-World')
    holding_type    TEXT NOT NULL DEFAULT 'stock',  -- 'stock' | 'etf' | 'fund' | 'bond' | 'crypto'
    quantity        TEXT NOT NULL,             -- Decimal string (shares/units)
    price_per_unit  TEXT,                      -- Decimal string, price at snapshot time
    value           TEXT NOT NULL,             -- Decimal string, total value (quantity * price)
    currency        TEXT NOT NULL DEFAULT 'GBP',
    as_of           TEXT NOT NULL,             -- YYYY-MM-DD, when this snapshot was taken
    created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    UNIQUE(account_id, symbol, as_of),
    FOREIGN KEY (account_id) REFERENCES accounts(id)
);

CREATE INDEX IF NOT EXISTS idx_holdings_account ON holdings(account_id);
CREATE INDEX IF NOT EXISTS idx_holdings_as_of   ON holdings(as_of);
CREATE INDEX IF NOT EXISTS idx_holdings_symbol  ON holdings(symbol);

-- ── ingestion_checklist ──────────────────────────────────────────────────
-- DISCUSS: Is this table needed for MVP? Nonso suggested starting small and
-- adding as needed. Could track ingestion status in-memory or derive from
-- import_log instead. See Open Questions in docs/fynance-project-note.md.
-- Tracks the guided monthly ingestion flow.
-- Each row represents whether an account has been updated for a given month.
CREATE TABLE IF NOT EXISTS ingestion_checklist (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    month           TEXT NOT NULL,             -- YYYY-MM
    account_id      TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'pending',  -- 'pending' | 'completed' | 'skipped'
    completed_at    TEXT,
    notes           TEXT,
    UNIQUE(month, account_id),
    FOREIGN KEY (account_id) REFERENCES accounts(id)
);

CREATE INDEX IF NOT EXISTS idx_checklist_month ON ingestion_checklist(month);

-- ── category_rules (deferred) ────────────────────────────────────────────
-- NOT included in the initial schema. Add this table later when the UI
-- needs to display or edit rules. For MVP, rules live only in
-- config/rules.yaml and are loaded into memory at startup.
--
-- CREATE TABLE IF NOT EXISTS category_rules (
--     id              INTEGER PRIMARY KEY AUTOINCREMENT,
--     pattern         TEXT NOT NULL,
--     category        TEXT NOT NULL,
--     priority        INTEGER NOT NULL DEFAULT 0,
--     source          TEXT NOT NULL DEFAULT 'yaml'  -- 'yaml' | 'user'
-- );
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
    pub is_recurring: bool,     // recurring transactions (salary, rent, subscriptions)
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
    pub month: String,          // YYYY-MM (DISCUSS: remove if standing budgets adopted)
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

#[derive(Debug, Clone)]
pub struct Holding {
    pub id: i64,
    pub account_id: String,
    pub symbol: String,
    pub name: String,
    pub holding_type: HoldingType,
    pub quantity: Decimal,
    pub price_per_unit: Option<Decimal>,
    pub value: Decimal,
    pub currency: String,
    pub as_of: NaiveDate,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HoldingType {
    Stock,
    Etf,
    Fund,
    Bond,
    Crypto,
    Cash,
}

#[derive(Debug, Clone)]
pub struct IngestionChecklistItem {
    pub month: String,          // YYYY-MM
    pub account_id: String,
    pub status: IngestionStatus,
    pub completed_at: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum IngestionStatus {
    Pending,
    Completed,
    Skipped,
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

5. **Point-in-time carry-forward for portfolio**: Both `portfolio_snapshots` and `holdings` store data as point-in-time snapshots. When querying a date where no data was recorded, the system carries forward the most recent prior value. For example, if a balance was recorded on January 15 and the next update is April 1, queries for February and March return the January value with a staleness indicator. The query pattern is:
   ```sql
   -- Get the latest known balance for each account as of a target date
   SELECT a.id, a.name, ps.balance, ps.snapshot_date
   FROM accounts a
   LEFT JOIN portfolio_snapshots ps ON ps.account_id = a.id
     AND ps.snapshot_date = (
       SELECT MAX(ps2.snapshot_date)
       FROM portfolio_snapshots ps2
       WHERE ps2.account_id = a.id
         AND ps2.snapshot_date <= ?1  -- target date
     )
   WHERE a.is_active = 1;
   ```
   The frontend displays "as of Jan 2023" when showing carried-forward data, so the user knows the value may be stale.

6. **Holdings for stock-level portfolio detail**: The `holdings` table stores per-symbol snapshots within investment accounts. This enables drill-down from "Trading 212: £14,310" to "VWRL: £8,000, AAPL: £3,200, ..." and further into ETF composition. Holdings use the same carry-forward semantics as account balances.

7. **No separate income table; recurring flag for projections**: Income is not stored in a dedicated table. Income transactions are regular transactions with a positive amount and a category under the `Income` parent (e.g., `Income: Salary`). Monthly income figures are derived by summing positive transactions in the Income category for a given month. The `is_recurring` flag distinguishes recurring income (salary, freelance retainers) from one-off income (gifts, refunds, bonuses). The UI should surface this split: "Recurring income: £4,000 | One-off: £500". The same flag applies to expenses (rent, subscriptions vs one-time purchases), enabling budget projections: "you spend ~£X/month on recurring costs" without needing a separate table.

8. **Holdings vs portfolio_snapshots**: These serve different levels of detail. `portfolio_snapshots` stores one balance per account per date for net worth trend charts (e.g., "Trading 212 was worth £14,310 on March 1"). `holdings` stores per-symbol detail within investment accounts (e.g., "VWRL: 50 units @ £160, AAPL: 20 units @ £160"). Portfolio snapshots aggregate up to net worth; holdings drill down into composition.

9. **Portfolio breakdowns are always derived, never stored**: Groupings like "by type" (savings, investments, pensions), "by institution" (Monzo, Trading 212), or "by currency" are computed at query time from the accounts and holdings tables. This avoids dual-write complexity: when a user adds a new holding, they only update one place, and all breakdowns update automatically. The API returns pre-computed aggregations; the frontend never aggregates raw data.

10. **Guided ingestion checklist**: The `ingestion_checklist` table tracks which accounts have been updated each month. When the user starts their monthly review, the app pre-populates a checklist of all active accounts with `status = 'pending'`. As each account is updated (via CSV import, screenshot, or manual balance update), the status flips to `completed`. The UI shows a progress indicator: "3 of 7 accounts updated for March 2026".

11. **Deferred tables**: The following tables are not in the MVP schema but are planned for future phases:
    - `api_tokens`: Bearer token storage for programmatic API access. Deferred until the token auth feature is built.
    - `settings`: Key-value store for user preferences (default currency, theme, etc.). Deferred to V1 (settings page).
    - `categories`: Persistent category table for UI editing. MVP reads from `config/rules.yaml`; API exposes read-only. Deferred to V1.
    - `stock_prices`: Historical price data for holdings valuation. MVP uses price at ingestion time only. Deferred to V1.

Open questions related to the data model are tracked in the project-level Open Questions section of `docs/fynance-project-note.md`.
