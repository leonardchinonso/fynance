-- fynance database schema.
-- This file is executed once at Db::open() time via execute_batch. Every
-- statement is idempotent (IF NOT EXISTS) so it can run safely on every
-- startup. Any breaking change here will need a proper migration step.

-- ── transactions ──────────────────────────────────────────────────────────
-- One row per imported bank transaction. Money is stored as TEXT (Decimal
-- as string) to avoid floating-point error. Positive = credit, negative =
-- debit. Every row carries a stable SHA-256 fingerprint so that repeat
-- imports of overlapping statements are idempotent.
CREATE TABLE IF NOT EXISTS transactions (
    id                   TEXT PRIMARY KEY,
    date                 TEXT NOT NULL,
    description          TEXT NOT NULL,
    normalized           TEXT NOT NULL,
    amount               TEXT NOT NULL,
    currency             TEXT NOT NULL DEFAULT 'GBP',
    account_id           TEXT NOT NULL,
    category             TEXT,
    category_id          TEXT,
    category_source      TEXT,
    confidence           REAL,
    notes                TEXT,
    is_recurring         INTEGER NOT NULL DEFAULT 0,
    exclude_from_summary INTEGER NOT NULL DEFAULT 0,
    fingerprint          TEXT NOT NULL UNIQUE,
    fitid                TEXT,
    source_document_ids  TEXT NOT NULL DEFAULT '[]',  -- JSON array of documents.id; provenance back to source files
    created_at           TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    FOREIGN KEY (category_id) REFERENCES categories(id)
);

CREATE INDEX IF NOT EXISTS idx_tx_date        ON transactions(date);
CREATE INDEX IF NOT EXISTS idx_tx_account     ON transactions(account_id);
CREATE INDEX IF NOT EXISTS idx_tx_category    ON transactions(category);
CREATE INDEX IF NOT EXISTS idx_tx_category_id ON transactions(category_id);
CREATE INDEX IF NOT EXISTS idx_tx_month       ON transactions(substr(date, 1, 7));
CREATE INDEX IF NOT EXISTS idx_tx_exclude_summary ON transactions(exclude_from_summary);

-- ── categories ───────────────────────────────────────────────────────────
-- Hierarchical category taxonomy. Parent categories (parent_id IS NULL)
-- exist for grouping; only leaf children are assignable to transactions.
-- Max depth: 2 (parent + child). Seeded from categories.yaml on first startup.
CREATE TABLE IF NOT EXISTS categories (
    id            TEXT PRIMARY KEY,
    name          TEXT NOT NULL UNIQUE,
    parent_id     TEXT,
    display_order INTEGER DEFAULT 0,
    is_active     INTEGER NOT NULL DEFAULT 1,
    description   TEXT,
    -- Semantic classification (CategoryType): spending | income_taxable |
    -- income_non_taxable | interest_taxable | interest_non_taxable |
    -- internal_transfer | donation_taxable | donation_non_taxable.
    category_type TEXT NOT NULL DEFAULT 'spending',
    created_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    FOREIGN KEY (parent_id) REFERENCES categories(id)
);

CREATE INDEX IF NOT EXISTS idx_categories_parent_id ON categories(parent_id);
CREATE INDEX IF NOT EXISTS idx_categories_active ON categories(is_active);

-- ── import_log ────────────────────────────────────────────────────────────
-- Append-only audit trail of every file / payload ingested. Used by the
-- stats command and, later, the UI, to show ingestion history.
CREATE TABLE IF NOT EXISTS import_log (
    id                    INTEGER PRIMARY KEY AUTOINCREMENT,
    filename              TEXT NOT NULL,
    account_id            TEXT NOT NULL,
    rows_total            INTEGER NOT NULL,
    rows_inserted         INTEGER NOT NULL,
    rows_duplicate        INTEGER NOT NULL,
    source                TEXT NOT NULL DEFAULT 'csv',
    detected_bank         TEXT,
    detection_confidence  REAL,
    imported_at           TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

-- ── accounts ──────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS accounts (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL,
    institution     TEXT NOT NULL,
    type            TEXT NOT NULL,   -- 'checking' | 'savings' | 'emergency_fund' | 'investment' | 'investment_isa' | 'credit' | 'pension' | 'property'
    currency        TEXT NOT NULL DEFAULT 'GBP',
    is_active       INTEGER NOT NULL DEFAULT 1,
    notes           TEXT,
    profile_ids     TEXT NOT NULL DEFAULT '[]'
);

-- ── budgets ───────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS budgets (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    month       TEXT NOT NULL,
    category    TEXT,
    category_id TEXT,
    amount      TEXT NOT NULL,
    UNIQUE(month, category_id),
    FOREIGN KEY (category_id) REFERENCES categories(id)
);

CREATE INDEX IF NOT EXISTS idx_budget_month ON budgets(month);

-- ── holdings ──────────────────────────────────────────────────────────────
-- Per-symbol detail within investment accounts. Also stores cash balances as
-- rows with symbol='_CASH' and holding_type='cash'. Carries forward by date.
CREATE TABLE IF NOT EXISTS holdings (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id      TEXT NOT NULL,
    symbol          TEXT NOT NULL,
    name            TEXT NOT NULL,
    holding_type    TEXT NOT NULL DEFAULT 'stock',  -- 'stock' | 'etf' | 'fund' | 'bond' | 'crypto' | 'cash' | 'property' | 'debt'
    quantity        TEXT NOT NULL,
    price_per_unit  TEXT,
    value           TEXT NOT NULL,
    currency        TEXT NOT NULL DEFAULT 'GBP',
    as_of           TEXT NOT NULL,
    short_name      TEXT,
    sub_account     TEXT,
    is_closed       INTEGER NOT NULL DEFAULT 0,
    source_document_ids TEXT NOT NULL DEFAULT '[]',  -- JSON array of documents.id
    created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    FOREIGN KEY (account_id) REFERENCES accounts(id)
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_holdings_identity
    ON holdings(account_id, symbol, COALESCE(sub_account, ''), as_of);
CREATE INDEX IF NOT EXISTS idx_holdings_account   ON holdings(account_id);
CREATE INDEX IF NOT EXISTS idx_holdings_as_of     ON holdings(as_of);
CREATE INDEX IF NOT EXISTS idx_holdings_symbol    ON holdings(symbol);
CREATE INDEX IF NOT EXISTS idx_holdings_is_closed ON holdings(is_closed);

-- ── investments ───────────────────────────────────────────────────────────
-- Immutable ledger of share acquisition and disposal events. One row per
-- event (vest, buy, sell, withhold, transfer, split). Never updated or deleted.
-- ISA accounts (type = 'investment_isa') are excluded from CGT calculations
-- but events are still stored here for record-keeping. S104 pool state and
-- CGT disposals are computed on the fly from this table — no separate cache tables.
-- GBP conversion for HMRC is computed at query time using historic FX rates:
-- proceeds use the disposal-date rate, costs use the acquisition-date rate.
CREATE TABLE IF NOT EXISTS investments (
    id               TEXT PRIMARY KEY,       -- UUID v4
    account_id       TEXT NOT NULL,
    event_type       TEXT NOT NULL,    -- 'vest' | 'buy' | 'sell' | 'transfer' | 'withhold' | 'split'
    symbol           TEXT NOT NULL,    -- ticker or ISIN (e.g. 'AAPL', 'VWRL'); name derived from holdings at query time
    date             TEXT NOT NULL,    -- ISO 8601 datetime (YYYY-MM-DDTHH:MM:SS); date-only imports use T00:00:00
    quantity         TEXT NOT NULL,    -- Decimal as TEXT (shares/units)
    price_per_share  TEXT NOT NULL,    -- Decimal as TEXT, in native currency
    fee              TEXT,             -- Decimal as TEXT; broker commission + stamp duty; currency given by fee_currency; NULL for splits/transfers
    currency         TEXT NOT NULL,    -- native currency of the price (e.g. 'USD', 'GBP')
    fee_currency     TEXT,             -- ISO 4217 currency of the fee; may differ from currency (e.g. USD-priced trade, GBP commission); NULL means same as currency / no fee
    notes            TEXT,
    fingerprint      TEXT NOT NULL UNIQUE,
    source_document_ids TEXT NOT NULL DEFAULT '[]',  -- JSON array of documents.id
    created_at       TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    FOREIGN KEY (account_id) REFERENCES accounts(id)
);

CREATE INDEX IF NOT EXISTS idx_investments_account ON investments(account_id);
CREATE INDEX IF NOT EXISTS idx_investments_symbol  ON investments(symbol);
CREATE INDEX IF NOT EXISTS idx_investments_date    ON investments(date);

-- ── ingestion_checklist ──────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS ingestion_checklist (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    month           TEXT NOT NULL,
    account_id      TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'pending',
    completed_at    TEXT,
    notes           TEXT,
    UNIQUE(month, account_id),
    FOREIGN KEY (account_id) REFERENCES accounts(id)
);

CREATE INDEX IF NOT EXISTS idx_checklist_month ON ingestion_checklist(month);

-- ── api_tokens ────────────────────────────────────────────────────────────
-- Bearer tokens used by scripts and external agents. We only store the
-- SHA-256 hash; the raw token is shown to the user exactly once at creation.
CREATE TABLE IF NOT EXISTS api_tokens (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT NOT NULL UNIQUE,
    token_hash  TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    last_used   TEXT,
    is_active   INTEGER NOT NULL DEFAULT 1
);

-- ── profiles ──────────────────────────────────────────────────────────────────
-- Represent people in a multi-person household. Accounts reference profiles
-- via the `profile_ids` JSON array column. Never auto-seeded: a fresh database
-- has zero profiles until one is created via the API/CLI (this is what lets a
-- deleted profile stay deleted across restarts).
-- See docs/plans/archive/12_frontend_backend_consolidation.md §Profile Semantics.
-- `utr` is the HMRC Unique Taxpayer Reference (10 digits). Nullable: it is only
-- needed to produce a filing-grade CGT report, and it is per-taxpayer rather
-- than per-household, which is why it hangs off profiles — the two profiles
-- file separately and CGT reports already scope by profile.
CREATE TABLE IF NOT EXISTS profiles (
    id   TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    utr  TEXT
);

-- ── standing_budgets ──────────────────────────────────────────────────────────
-- One standing monthly target per category. Applies to every month unless
-- a budget_overrides row exists for that (month, category_id) pair.
CREATE TABLE IF NOT EXISTS standing_budgets (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    category    TEXT,
    category_id TEXT,
    amount      TEXT NOT NULL,
    UNIQUE(category_id),
    FOREIGN KEY (category_id) REFERENCES categories(id)
);

-- ── budget_overrides ──────────────────────────────────────────────────────────
-- Per-month overrides on top of standing budgets (e.g. higher food budget
-- in December). COALESCE(override.amount, standing.amount) is the effective value.
CREATE TABLE IF NOT EXISTS budget_overrides (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    month       TEXT NOT NULL,
    category    TEXT,
    category_id TEXT,
    amount      TEXT NOT NULL,
    UNIQUE(month, category_id),
    FOREIGN KEY (category_id) REFERENCES categories(id)
);

-- ── documents ─────────────────────────────────────────────────────────────
-- First-class store for every uploaded source file. The bytes live on disk in
-- a `documents/` subdirectory beside the DB; this table holds the metadata and
-- the path. `content_hash` is unique so re-uploading or re-parsing the same
-- bytes reuses the existing row instead of creating a duplicate (the main
-- defence against orphaned files piling up across a session). Provenance back
-- to individual rows lives in the `source_document_ids` JSON array on
-- transactions / holdings / investments, not here.
CREATE TABLE IF NOT EXISTS documents (
    id           TEXT PRIMARY KEY,              -- UUID v4
    filename     TEXT NOT NULL,                 -- original uploaded filename
    file_path    TEXT NOT NULL,                 -- absolute path on disk
    mime_type    TEXT NOT NULL,                 -- e.g. text/csv, application/pdf, image/png
    size_bytes   INTEGER NOT NULL,
    content_hash TEXT NOT NULL,                 -- sha256 of the bytes, for dedup
    origin       TEXT NOT NULL DEFAULT 'parse', -- 'parse' | 'manual' (informational only)
    account_id   TEXT,                          -- best-effort link to the import account; nullable
    uploaded_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_documents_hash ON documents(content_hash);
CREATE INDEX IF NOT EXISTS idx_documents_account ON documents(account_id);

-- ── currencies ────────────────────────────────────────────────────────────
-- App-level list of supported currencies. Code is the PK (ISO 4217).
-- Exactly one row has is_preferred=1 at all times.
-- Constraints enforced at application layer.
CREATE TABLE IF NOT EXISTS currencies (
    code            TEXT PRIMARY KEY,                -- ISO 4217, e.g. 'GBP', 'NGN'
    is_preferred    INTEGER NOT NULL DEFAULT 0,      -- 1 for preferred, 0 for others
    fx_rate         TEXT NOT NULL,                   -- Decimal string. '1' for preferred.
    updated_at      TEXT                             -- nullable: NULL for preferred row
);

-- ── exchange_rates ────────────────────────────────────────────────────────
-- Date-keyed FX rates, used by the CGT engine to convert each leg of a
-- disposal at its own date's rate (HMRC requires the acquisition cost at the
-- acquisition-date rate and the proceeds at the disposal-date rate; converting
-- the resulting gain is explicitly not allowed).
--
-- RATE DIRECTION — the single most important thing about this table:
--   `rate` is the number of `quote` units you get for ONE `base` unit, i.e.
--       amount_in_quote = amount_in_base * rate
--   So a row (base='USD', quote='GBP', rate='0.7862') means $1 = £0.7862, and
--   converting $100 to GBP is 100 * 0.7862 = £78.62.
--   This matches the direction of `currencies.fx_rate`, which is likewise a
--   multiplier onto the preferred currency — the two never disagree about
--   which way round a rate points. An inverted rate produces plausible-looking
--   but wrong tax numbers, which is the exact failure this table exists to
--   prevent, hence spelling it out here rather than in a commit message.
--
-- These rates are USER-OWNED. Nothing auto-fetches or interpolates them: HMRC
-- mandates no particular rate source, only that the chosen basis is applied
-- consistently, and the main use case is reproducing the rates a previously
-- filed return was computed with. A missing rate is therefore a prompt to the
-- user, never a silent fallback — so weekends and bank holidays need no gap
-- policy.
--
-- `source` records provenance so a generated report can show which rate was
-- used and where it came from: 'user' (typed in) or 'suggested' (pre-filled by
-- a provider and then accepted by the user — the stored value is always what
-- the user committed).
CREATE TABLE IF NOT EXISTS exchange_rates (
    base       TEXT NOT NULL,                        -- currency being converted FROM, e.g. 'USD'
    quote      TEXT NOT NULL,                        -- currency being converted TO, e.g. 'GBP'
    date       TEXT NOT NULL,                        -- YYYY-MM-DD, the date the rate applies to
    rate       TEXT NOT NULL,                        -- Decimal string; quote units per 1 base unit
    source     TEXT NOT NULL DEFAULT 'user',         -- 'user' | 'suggested'
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    PRIMARY KEY (base, quote, date)
);

CREATE INDEX IF NOT EXISTS idx_exchange_rates_date ON exchange_rates(date);
