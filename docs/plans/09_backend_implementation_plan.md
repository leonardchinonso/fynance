# Backend MVP Implementation Plan

This is the executable checklist for building the fynance backend. It is split into self-contained phases, each of which produces a working vertical slice before the next begins. Every phase references the canonical design documents in `docs/design/` and the architecture overview in `docs/plans/08_mvp_phases_v2.md`.

> **How to use this document**: Work through each phase top-to-bottom. Check off tasks as they are completed. Each phase has a clear deliverable that can be verified before moving on. The phases are intentionally ordered so that earlier work is never thrown away -- each phase builds on the previous one.

---

## Phase 1: Project Foundation + Data Layer

**Goal**: A working Rust binary that reads UK bank CSV exports (Monzo, Revolut, Lloyds) and stores transactions in SQLite. No HTTP server, no UI, no async runtime.

**Reference**: `docs/plans/08_mvp_phases_v2.md` Phase 1, `docs/design/03_data_model.md`

### 1.1 Repository scaffold

- [ ] Create `backend/` directory with `Cargo.toml`
- [ ] Create `db/sql/schema.sql` containing all tables from `docs/design/03_data_model.md`:
  - `transactions` with all indexes
  - `import_log`
  - `accounts`
  - `portfolio_snapshots` with index
  - `budgets` with index
  - `holdings` with index
  - `ingestion_checklist`
  - `api_tokens`
- [ ] Create `backend/config/categories.yaml` with the full category taxonomy (top-level categories and subcategories, e.g. `Food: Groceries`, `Food: Eating Out`, `Transport: Commute`, etc.)
- [ ] Create `.env.example` documenting all environment variables: `FYNANCE_PORT`, `FYNANCE_DB_PATH`, `FYNANCE_HOST`, `FYNANCE_LOG_LEVEL`
- [ ] Create `Makefile` with targets: `build` (frontend then cargo), `dev-backend`, `dev-frontend`, `test`, `lint`, `fmt`
- [ ] Add `Cargo.toml` Phase 1 dependencies (see `08_mvp_phases_v2.md` Phase 1 for the exact list)

### 1.2 Core types (`backend/src/model.rs`)

- [ ] `Transaction` struct with all fields from schema; derives `serde::Serialize/Deserialize`, `ts_rs::TS`, `#[ts(export)]`
- [ ] `Account` struct; same derives
- [ ] `AccountType` enum: `Checking | Savings | Investment | Credit | Cash | Pension`
- [ ] `CategorySource` enum: `Rule | Agent | Manual`
- [ ] `Budget` struct
- [ ] `PortfolioSnapshot` struct
- [ ] `Holding` struct
- [ ] `HoldingType` enum: `Stock | Etf | Fund | Bond | Crypto`
- [ ] `ImportResult` struct: `{ rows_total, rows_inserted, rows_duplicate, filename, account_id }`
- [ ] `InsertOutcome` enum: `Inserted | Duplicate`

### 1.3 Utilities (`backend/src/util.rs`)

- [ ] `normalize_description(raw: &str) -> String` -- strips noise tokens, lowercases, trims
- [ ] `fingerprint(date: &str, amount: &str, description: &str, account_id: &str) -> String` -- `sha256(joined fields)` as hex string
- [ ] `parse_date(s: &str) -> Result<NaiveDate>` -- handles `YYYY-MM-DD` and `DD/MM/YYYY`

### 1.4 Storage layer (`backend/src/storage/db.rs`)

- [ ] `Db` struct wrapping `rusqlite::Connection`
- [ ] `Db::open(path: &Path) -> Result<Db>`:
  - Resolves path from `dirs::data_local_dir()` when no explicit path given
  - Creates parent directory with mode `0o700` (Unix) if it does not exist
  - Sets DB file to mode `0o600` (Unix) on creation
  - Runs `schema.sql` via `execute_batch`
  - Enables WAL mode: `PRAGMA journal_mode=WAL`
- [ ] `Db::insert_transaction(&self, tx: &Transaction) -> Result<InsertOutcome>` -- INSERT OR IGNORE on fingerprint
- [ ] `Db::log_import(&self, log: &ImportLog) -> Result<()>`
- [ ] `Db::get_accounts(&self) -> Result<Vec<Account>>`
- [ ] `Db::upsert_account(&self, account: &Account) -> Result<()>`
- [ ] `Db::get_transactions(&self, filters: &TransactionFilters) -> Result<Vec<Transaction>>` -- filters: month, category, account, page, limit
- [ ] `Db::update_transaction_category(&self, id: &str, category: &str, source: CategorySource) -> Result<()>`
- [ ] `Db::set_budget(&self, month: &str, category: &str, amount: Decimal) -> Result<()>` -- upsert
- [ ] `Db::get_budget(&self, month: &str) -> Result<Vec<BudgetRow>>` -- joins with actual spending
- [ ] `Db::upsert_portfolio_snapshot(&self, snapshot: &PortfolioSnapshot) -> Result<()>`
- [ ] `Db::get_portfolio_as_of(&self, date: NaiveDate) -> Result<Vec<PortfolioRow>>` -- carry-forward query (see design/03_data_model.md Key Decision 5)
- [ ] `Db::upsert_holdings(&self, account_id: &str, holdings: &[Holding]) -> Result<()>`

### 1.5 Import trait and CSV importers

> **Iteration note (prompt 3.3).** Implement this section per [`10_llm_csv_import.md`](10_llm_csv_import.md), not the header-sniffing bullets below. The bullets are kept for historical context so the diff against the original Phase 1 is reviewable. In short: there is no `detect_format`, no per-bank column mapping, and no per-bank branch inside `map_row`. `CsvImporter` is a thin adapter around `LlmStatementParser`, which produces `UnifiedStatementRow`s and a `(detected_bank, detection_confidence)` tag. Two confidence gates apply: file-level (hard fail below threshold) and row-level (skip + warn). Unknown banks pass through as `BankFormat::Unknown` provided file-level confidence clears the threshold.

- [ ] `backend/src/importers/mod.rs`:
  - `Importer` trait: `fn import(&self, path: &Path, account_id: &str, db: &Db) -> Result<ImportResult>`
  - `get_importer(path: &Path) -> Result<Box<dyn Importer>>` -- extension-based dispatch only (`.csv` -> `CsvImporter`)
- [ ] `backend/src/importers/csv_importer.rs`:
  - `BankFormat` enum: `Monzo | Revolut | Lloyds | Unknown`
  - `detect_format(headers: &StringRecord) -> BankFormat`
  - `CsvImporter` implementing `Importer` for all three formats
  - Monzo: columns `Transaction ID, Date, Name, Category, Amount` -- amount already signed
  - Revolut: columns `Type, Completed Date, Description, Amount` -- amount already signed
  - Lloyds: columns `Transaction Date, Transaction Description, Debit Amount, Credit Amount` -- separate columns, `DD/MM/YYYY` date
  - For each row: parse date + amount, normalize description, compute fingerprint, call `db.insert_transaction`
  - Show `indicatif` progress bar during import

### 1.6 CLI commands

- [ ] `backend/src/commands/import.rs`:
  - Accepts a single file path or directory
  - For directories: glob `*.csv`, process all files in parallel with Rayon (or sequentially for Phase 1)
  - Prints per-file summary: `Imported 142 new, 3 duplicates (monzo_march.csv)`
  - Records each file in `import_log`
- [ ] `backend/src/commands/stats.rs`:
  - Total transaction count and date range
  - Per-account breakdown (count, date range, uncategorized count)
- [ ] `backend/src/commands/account.rs`:
  - `account add --id --name --institution --type [--balance] [--currency]`
  - `account set-balance <id> <amount> --date YYYY-MM-DD`
  - `account list`
- [ ] `backend/src/commands/budget.rs` (stub for Phase 1, flesh out in Phase 3):
  - `budget set --month YYYY-MM --category <cat> --amount N`
  - `budget status`
- [ ] `backend/src/cli.rs`: clap derive macros, top-level `Cli` and `Commands` enum
- [ ] `backend/src/main.rs`: dispatch to commands, init tracing-subscriber with `FYNANCE_LOG_LEVEL`

### 1.7 Tests

- [ ] Unit tests in `util.rs`: `normalize_description`, `fingerprint`, `parse_date` (both date formats, invalid input)
- [ ] Integration test: import a fixture Monzo CSV, verify row count, deduplication, and amount sign
- [ ] Integration test: import a fixture Revolut CSV
- [ ] Integration test: import a fixture Lloyds CSV

### Deliverable

```
cargo run -- import monzo_march.csv --account monzo-current
# Imported 142 new, 0 duplicates (monzo_march.csv) in 0.3s

cargo run -- stats
# Total: 1,842 transactions (2024-01-01 to 2026-04-10)
# monzo-current: 1,120 | revolut-main: 722 | lloyds-current: 0
```

---

## Phase 2: HTTP Server + Auth + Frontend Shell

**Goal**: A running Axum server that serves the compiled React app and exposes all REST routes. Browser opens automatically on `fynance serve`. API token generation works via CLI. No real data in the UI yet.

**Reference**: `docs/plans/08_mvp_phases_v2.md` Phase 2, `docs/design/05_security_isolation.md`

### 2.1 Axum server scaffold

- [ ] Add to `Cargo.toml`: `axum`, `tokio` (features: full), `tower-http` (features: cors, fs), `include_dir`, `open`, `dotenvy`
- [ ] `backend/src/server/mod.rs`:
  - `build_router(db: Arc<Db>) -> Router` -- assembles all routes with shared state
  - CORS: `tower_http::cors::CorsLayer::permissive()` (loopback only, not a security boundary)
  - Static file fallback: serve embedded `frontend/dist/` via `include_dir!`; all unmatched GET routes return `index.html` for client-side routing
- [ ] `backend/src/server/static_files.rs`:
  - `static FRONTEND_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/../frontend/dist")`
  - `serve_static(uri: &str) -> Response` -- look up file in `FRONTEND_DIR`, return with correct `Content-Type`; fallback to `index.html`
- [ ] `backend/src/commands/serve.rs`:
  - Load `.env` via `dotenvy`
  - Bind `TcpListener` to `FYNANCE_HOST:FYNANCE_PORT` (default `127.0.0.1:7433`)
  - Start Axum server on the listener
  - Call `open::that("http://localhost:7433")` unless `--no-open` flag is set
  - Log startup: `fynance: server started at http://localhost:7433`
- [ ] Add `serve` subcommand to `src/cli.rs`

### 2.2 API token authentication

Schema (already in `db/sql/schema.sql` from Phase 1):
```sql
CREATE TABLE IF NOT EXISTS api_tokens (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT NOT NULL UNIQUE,
    token_hash  TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    last_used   TEXT,
    is_active   INTEGER NOT NULL DEFAULT 1
);
```

- [ ] `Db::create_token(&self, name: &str) -> Result<String>` -- generate `fyn_` + 32 random hex bytes, store `SHA-256(token)` in DB, return raw token (shown once)
- [ ] `Db::list_tokens(&self) -> Result<Vec<TokenInfo>>`
- [ ] `Db::revoke_token(&self, name: &str) -> Result<()>` -- set `is_active = 0`
- [ ] `Db::validate_token(&self, raw_token: &str) -> Result<bool>` -- hash and lookup; update `last_used`
- [ ] `backend/src/server/auth.rs`:
  - `AuthLayer` middleware: extract `Authorization: Bearer fyn_...` header
  - Requests from loopback (`127.0.0.1`) without a token are allowed (browser UI path)
  - Requests with a valid token are allowed (programmatic API path)
  - All other requests get `401 Unauthorized`
  - Attach `AuthContext` (anonymous loopback vs. authenticated token) to request extensions
- [ ] CLI token commands in `backend/src/commands/token.rs`:
  - `token create --name <name>` -- prints raw token once
  - `token list`
  - `token revoke --name <name>`

### 2.3 API docs endpoint (`GET /api/docs`)

- [ ] `backend/src/server/routes/docs.rs`:
  - Returns OpenAPI JSON spec (hand-crafted for MVP, `ts-rs`-assisted later)
  - Must include: all route definitions, request/response schemas, field descriptions, example payloads, full category taxonomy from `config/categories.yaml`, `category_source` field values and their meaning
  - Designed to be agent-readable: an AI agent should be able to fetch this and immediately start pushing data
- [ ] Register route: `GET /api/docs` -- no auth required

### 2.4 Frontend shell

- [ ] Initialize React 19 project in `frontend/` with Vite + TypeScript template
- [ ] Install and configure: React 19, React Compiler (babel-plugin-react-compiler), Tailwind CSS, Recharts, shadcn-ui base components
- [ ] Create `frontend/src/App.tsx` with a four-tab layout: Transactions, Budget, Portfolio, Reports
- [ ] Create `frontend/src/components/Navbar.tsx` with tab navigation
- [ ] Create `frontend/src/components/DateRangePicker.tsx`: always-visible date range selector with presets (current month, last 3 months, YTD, full year, last 5 years, custom)
- [ ] Create `frontend/src/context/DateRangeContext.tsx`: shared date range state across all pages
- [ ] Create placeholder `frontend/src/pages/Transactions.tsx`, `Budget.tsx`, `Portfolio.tsx`, `Reports.tsx` (each renders "Coming soon.")
- [ ] Create `frontend/src/api/client.ts`: typed fetch wrappers for all API routes; returns mock data for now, real fetch later
- [ ] Create `frontend/src/data/mock.ts`: typed mock data matching Rust-generated types from `frontend/src/bindings/`
- [ ] `cargo test` generates TypeScript bindings into `frontend/src/bindings/` via `ts-rs`
- [ ] `npm run build` in `frontend/` produces `frontend/dist/`; `cargo build --release` embeds it

### 2.5 Error handling conventions

- [ ] `backend/src/server/error.rs`:
  - `AppError` enum with variants for `NotFound`, `BadRequest`, `Unauthorized`, `Internal`
  - Implement `IntoResponse` for `AppError`: maps to appropriate HTTP status + JSON body `{ "error": "..." }`
  - All Axum handlers return `Result<Json<T>, AppError>`

### Deliverable

```
fynance serve
# fynance: server started at http://localhost:7433
# (default browser opens)
```

Browser shows navbar with four tabs. All tabs display "Coming soon." API token CLI works:

```
fynance token create --name "import-script"
# Token: fyn_a1b2c3... (shown once, store securely)

fynance token list
# import-script  created 2026-04-11  last_used never
```

---

## Phase 3: Transactions API + Budget API

**Goal**: Real transaction data visible in the browser. Users can filter transactions, see budget vs actual, and set budget targets. The guided ingestion checklist flow works.

**Reference**: `docs/plans/08_mvp_phases_v2.md` Phase 3

### 3.1 Transactions routes

- [ ] `GET /api/transactions`:
  - Query params: `month` (YYYY-MM), `category`, `account_id`, `page` (default 1), `limit` (default 50)
  - Response: `{ transactions: Transaction[], total: number, page: number, total_pages: number }`
  - Requires: `Db::get_transactions` with pagination and filter support
- [ ] `GET /api/transactions/categories`:
  - Returns distinct categories present in the DB, plus all categories from `config/categories.yaml`
  - Response: `string[]`
- [ ] `GET /api/transactions/accounts`:
  - Returns all accounts with transaction count
  - Response: `{ id, name, institution, type, transaction_count }[]`
- [ ] `PATCH /api/transactions/:id`:
  - Body: `{ category?: string, notes?: string }`
  - Updates category (source set to `manual`) and/or notes
  - Response: updated `Transaction`

### 3.2 Import routes (programmatic API)

- [ ] `POST /api/import`:
  - Body: `{ account_id: string, transactions: ImportTransaction[] }` where `ImportTransaction` has `date, description, amount, currency, category?, category_source?`
  - Auth required (API token)
  - Runs same dedup fingerprint logic as CSV importer
  - Response: `ImportResult`
- [ ] `POST /api/import/csv`:
  - Multipart form: `file` (single CSV), `account` (account_id string)
  - Auth required (API token)
  - Reuses CSV importer logic from Phase 1
  - Response: `ImportResult`
- [ ] `POST /api/import/bulk`:
  - Multipart form: `files[]` (multiple CSVs), `accounts[]` (account_id per file, same order)
  - Auth required (API token)
  - Processes each file, returns aggregated results
  - Response: `ImportResult[]`

### 3.3 Budget routes

- [ ] `GET /api/budget/:month`:
  - Returns all categories with `{ category, budgeted, actual, delta }` for the given month
  - `actual` is sum of absolute values of negative transactions in that category and month
  - `budgeted` is NULL if no budget is set for that category
  - Response: `BudgetRow[]`
- [ ] `POST /api/budget`:
  - Body: `{ month: string, category: string, amount: string }`
  - Upserts a budget row
  - Response: `{ ok: true }`
- [ ] `GET /api/income/:month`:
  - Derived from transactions in the `Income` category for the given month
  - Response: `{ month: string, total: string, transactions: Transaction[] }`

### 3.4 Guided ingestion checklist routes

- [ ] Storage: `Db::get_ingestion_checklist(month: &str) -> Result<Vec<ChecklistRow>>`
- [ ] Storage: `Db::upsert_checklist_item(month: &str, account_id: &str, status: &str) -> Result<()>`
- [ ] `GET /api/ingestion/checklist/:month`:
  - Returns all active accounts with their status for the month: `{ account_id, name, status: 'pending' | 'completed' | 'skipped', updated_at? }[]`
- [ ] `POST /api/ingestion/checklist/:month/:account_id`:
  - Body: `{ status: 'completed' | 'skipped' }`
  - Updates or inserts a checklist row
  - Response: updated row

### 3.5 Storage additions (beyond Phase 1)

- [ ] `Db::get_transactions(&self, filters: TransactionFilters) -> Result<(Vec<Transaction>, u64)>` -- returns (rows, total_count) for pagination
- [ ] `Db::count_transactions(&self, filters: TransactionFilters) -> Result<u64>`
- [ ] `Db::get_budget_vs_actual(&self, month: &str) -> Result<Vec<BudgetRow>>` -- LEFT JOIN budgets + GROUP BY on transactions
- [ ] `Db::get_income(&self, month: &str) -> Result<(Decimal, Vec<Transaction>)>`
- [ ] `Db::insert_transactions_bulk(&self, txns: &[Transaction]) -> Result<ImportResult>` -- batch insert for API import

### 3.6 Frontend: Transactions page

- [ ] Wire `client.ts` functions to real API endpoints (remove mock data for transactions, budget)
- [ ] `frontend/src/pages/Transactions.tsx`:
  - Date range selector at top (from `DateRangeContext`)
  - Account multi-select filter
  - Category multi-select filter
  - **Table view**: date, merchant (normalized), category badge, amount, confidence indicator
  - Inline category edit: click category badge, dropdown with all categories, saves via `PATCH /api/transactions/:id`
  - Pagination controls
  - **Bar chart view**: spending per category for the selected period (Recharts `BarChart`)
  - **Pie chart view**: category breakdown with hover tooltips (Recharts `PieChart`)
  - View mode toggle (table / bar / pie)
  - Export button: CSV, image

### 3.7 Frontend: Budget page

- [ ] `frontend/src/pages/Budget.tsx`:
  - Date range selector at top
  - Income bar: budgeted vs actual income
  - Category rows: progress bar + amounts + color coding (green < 80%, amber 80-110%, red > 110%)
  - "Edit budget" mode: inline amount inputs per category
  - **Table view**: spreadsheet-style, category x month grid
  - **Stacked bar chart view**: spending by category over time
  - **Line chart view**: spending trends per category
  - **Pie chart view**: category breakdown for period, interactive
  - Budget planning mode: hover a category to see last 12-month average

### Deliverable

Full transaction list in the browser with filtering, pagination, inline category editing. Budget tab shows real spending per category with color coding. User can set budget amounts per category via the UI.

---

## Phase 4: Portfolio API + Portfolio UI

**Goal**: Net worth overview with account balances, point-in-time carry-forward, stock-level holdings for investment accounts, diversity breakdown charts, and full filter support.

**Reference**: `docs/plans/08_mvp_phases_v2.md` Phase 4, `docs/design/04_portfolio_overview.md`, `docs/design/03_data_model.md`

### 4.1 Portfolio routes

- [ ] `GET /api/portfolio`:
  - Returns all active accounts with their latest known balance (carry-forward semantics)
  - `?as_of=YYYY-MM-DD` optional param: uses most recent snapshot on or before that date
  - For each account, includes a `staleness` field: `{ stale: bool, as_of_date: string }` if the latest snapshot is older than the requested date
  - Response: `PortfolioView` with `{ total_net_worth, accounts: AccountWithBalance[], as_of_date }`
- [ ] `GET /api/portfolio/history`:
  - `?from=YYYY-MM-DD&to=YYYY-MM-DD` date range
  - Returns monthly net worth snapshots for the range
  - Response: `{ date: string, net_worth: string }[]`
- [ ] `POST /api/accounts`:
  - Body: `{ id, name, institution, type, currency?, balance?, balance_date?, notes? }`
  - Creates or updates an account
  - If `balance` is provided, also inserts a `portfolio_snapshots` row
  - Response: `Account`
- [ ] `PATCH /api/accounts/:id/balance`:
  - Body: `{ balance: string, date: string }`
  - Updates `accounts.balance` and `accounts.balance_date`
  - Inserts/updates a row in `portfolio_snapshots` for the given date
  - Response: updated `Account`
- [ ] `GET /api/holdings/:account_id`:
  - Returns all holdings for an investment account with `{ symbol, name, holding_type, quantity, value_gbp, allocation_pct }`
  - Response: `Holding[]`
- [ ] `POST /api/holdings/:account_id`:
  - Body: `Holding[]` -- full replacement (upsert all, mark missing as sold if not present)
  - Used for bulk update from a platform export
  - Auth required (API token)
  - Response: `{ ok: true, holdings_updated: number }`

### 4.2 Carry-forward query logic

The core portfolio query must implement the carry-forward pattern from `docs/design/03_data_model.md` Key Decision 5:

- [ ] `Db::get_portfolio_as_of(&self, as_of: NaiveDate) -> Result<Vec<PortfolioRow>>`:
  ```sql
  -- For each account, get the most recent snapshot on or before `as_of`
  SELECT
      a.id, a.name, a.institution, a.type,
      ps.balance, ps.snapshot_date AS as_of_date,
      ps.snapshot_date < ?1 AS is_stale
  FROM accounts a
  LEFT JOIN portfolio_snapshots ps ON ps.account_id = a.id
      AND ps.snapshot_date = (
          SELECT MAX(snapshot_date)
          FROM portfolio_snapshots
          WHERE account_id = a.id
            AND snapshot_date <= ?1
      )
  WHERE a.is_active = 1
  ORDER BY a.institution, a.name;
  ```
- [ ] `Db::get_monthly_net_worth(&self, from: NaiveDate, to: NaiveDate) -> Result<Vec<NetWorthPoint>>` -- generates a point per month using carry-forward for months without a snapshot

### 4.3 CLI additions

- [ ] `fynance account add` (flesh out stub from Phase 1)
- [ ] `fynance account set-balance <id> <amount> --date YYYY-MM-DD`
- [ ] `fynance account list`

### 4.4 Frontend: Portfolio page

- [ ] `frontend/src/pages/Portfolio.tsx`:
  - Date range selector at top
  - **Net Worth card**: headline figure + delta from previous period + sparkline
  - **Accounts grid**: card per account with balance, type badge, institution, "Update balance" button, staleness indicator ("as of Jan 2026")
  - **Filter toggles**: multi-select checkboxes to include/exclude accounts, account types, institutions
  - **Multiple view modes**:
    - Table: all accounts, sortable by balance / type / institution
    - Net worth line chart (Recharts `LineChart`)
    - Allocation pie/donut charts: by account type, by institution (Recharts `PieChart`)
    - Cash flow bar chart: income vs spending per month
    - Stacked area chart: account balance composition over time
  - **Investment account drill-down**: click an investment account to expand and show holdings table (symbol, name, quantity, value, allocation %)
  - All charts update reactively when date range or filter toggles change
  - Export any view

### Deliverable

Portfolio tab shows net worth, all account balances with staleness indicators, stock-level holdings for investment accounts, allocation charts, and filter toggles. Point-in-time carry-forward works correctly for months without new data.

---

## Phase 5: Reports + Export

**Goal**: Exportable monthly summaries, data-driven reports, the `fynance monthly` composite command, and Obsidian-compatible markdown export.

**Reference**: `docs/plans/08_mvp_phases_v2.md` Phase 6

### 5.1 Reports route

- [ ] `GET /api/reports/:month`:
  - Computes a data-driven summary for the given month (no AI narrative for MVP)
  - Response: `MonthlyReport` with:
    - `total_spending`, `total_income`, `net`
    - `spending_by_category: { category, amount, budget, pct_of_budget }[]` sorted descending
    - `top_merchants: { name, amount, count }[]` top 10
    - `net_worth_snapshot`: carry-forward portfolio total as of month-end
    - `budget_status: 'under' | 'on_track' | 'over'`
    - `month_over_month: { spending_delta_pct, income_delta_pct }` vs previous month

### 5.2 Export route

- [ ] `GET /api/export?year=YYYY&format=csv`:
  - Returns all transactions for the year as a CSV file with `Content-Disposition: attachment`
  - Columns: `date, description, normalized, amount, currency, account, category, notes`
- [ ] `GET /api/export?month=YYYY-MM&format=md`:
  - Generates Obsidian-compatible markdown monthly summary (see `08_mvp_phases_v2.md` Phase 6 for format)
  - Response: markdown text with `Content-Type: text/markdown`
- [ ] `GET /api/export?year=YYYY&format=md`:
  - Generates a yearly summary in markdown format

### 5.3 CLI composite command

- [ ] `backend/src/commands/monthly.rs`:
  - `fynance monthly` -- orchestrates: `import` (all CSVs from a configured download dir) then `fynance account` balance prompts
  - Reads configured import directory from `.env` (`FYNANCE_IMPORT_DIR`)
  - Prints a checklist of what was processed
- [ ] `--dry-run` flag on `fynance import`: parses and deduplicates in memory, prints what would be inserted, writes nothing to DB

### 5.4 Frontend: Reports page

- [ ] `frontend/src/pages/Reports.tsx`:
  - Month selector (single month, not range)
  - Summary cards: total spending, total income, net, net worth
  - Category breakdown table with budget comparison
  - Top merchants list
  - Month-over-month delta indicators
  - Export button (CSV, markdown)

### Deliverable

Reports tab shows a full monthly breakdown. `fynance export` CLI and API endpoint produce downloadable files. `fynance monthly` runs the end-to-end ingestion workflow. Obsidian markdown export format works.

---

## Phase 6: Polish + Docker + CI

**Goal**: The app is ready for regular daily use. Docker deployment works. CI runs tests and linting on every push.

**Reference**: `docs/design/05_security_isolation.md`

### 6.1 Error handling and resilience

- [ ] All `AppError` variants return structured JSON `{ "error": "<message>", "code": "<slug>" }`
- [ ] All import routes return partial success with errors: `{ inserted, duplicates, errors: [{ row, reason }] }` rather than failing the whole request on a single bad row
- [ ] CSV auto-detection returns a clear error if the format is unrecognized, with a message showing the detected headers
- [ ] All DB operations use transactions for multi-insert operations (bulk import, portfolio update)

### 6.2 Configuration

- [ ] Load all config from `.env` via `dotenvy` at startup in `serve` and `import` commands
- [ ] `config/categories.yaml` is embedded in the binary via `include_str!` (read at startup, cached in a `OnceLock`)
- [ ] Config file at `~/.config/fynance/config.yaml` for user preferences (optional, overrides env vars)
- [ ] Config file created with mode `0o600` if it does not exist

### 6.3 Logging

- [ ] `tracing-subscriber` initialized with `FYNANCE_LOG_LEVEL` (default `info`)
- [ ] HTTP request logging via `tower_http::trace::TraceLayer` at `debug` level
- [ ] Never log raw transaction descriptions at `info` level (merchant names may be sensitive)
- [ ] Structured log fields: `account_id`, `month`, `rows_inserted`, `rows_duplicate` on import

### 6.4 Docker

- [ ] `Dockerfile` -- multi-stage:
  - Stage 1 (`node`): build frontend (`npm run build`)
  - Stage 2 (`rust`): copy frontend dist, build Rust binary with embedded frontend
  - Stage 3 (`debian-slim`): copy binary only, expose `FYNANCE_PORT`
- [ ] `docker-compose.yml`:
  - Service: `fynance`, build from `Dockerfile`
  - Volume: `./data:/data` for the SQLite DB
  - Environment: `FYNANCE_HOST=0.0.0.0`, `FYNANCE_DB_PATH=/data/fynance.db`, `FYNANCE_PORT=7433`
  - Port mapping: `7433:7433`
- [ ] `.dockerignore`: exclude `target/`, `node_modules/`, `.env`

### 6.5 CI/CD (`.github/workflows/`)

- [ ] `ci.yml` -- triggers on push and PR to `main`:
  - `cargo fmt --check`
  - `cargo clippy --all-targets -- -D warnings`
  - `cargo test`
  - Frontend: `npm ci && npm run build && npm run typecheck`
- [ ] `docker.yml` -- triggers on push to `main`:
  - Build and push Docker image to GHCR (`ghcr.io/leonardchinonso/fynance:latest`)
  - Tag with short SHA for rollback

### 6.6 Final verification checklist

- [ ] `fynance serve` opens browser, all four tabs render real data
- [ ] CSV import works for Monzo, Revolut, and Lloyds files
- [ ] Deduplication prevents double-imports
- [ ] API token auth blocks unauthenticated programmatic requests
- [ ] Budget tab shows color-coded spending with editable targets
- [ ] Portfolio shows net worth with carry-forward for stale months
- [ ] Reports tab generates correct monthly summaries
- [ ] Export endpoints return correct CSV and markdown
- [ ] Docker container starts cleanly and serves the UI on port 7433
- [ ] CI passes all checks

### Deliverable

The project is ready for regular use. Docker image published to GHCR. Any contributor can run `docker compose up` and immediately use the app with their own isolated database.

---

## Phase Summary

| Phase | Goal | Key Deliverable |
|---|---|---|
| 1 | Project scaffold + CSV import + SQLite | `fynance import` stores transactions; `fynance stats` reports counts |
| 2 | Axum server + React shell + API tokens | Browser opens at localhost:7433 with four placeholder tabs |
| 3 | Transactions + Budget API + guided ingestion | Real data in browser; budget vs actual with color coding |
| 4 | Portfolio API + carry-forward + holdings | Net worth view with staleness indicators and investment drill-down |
| 5 | Reports + Export + monthly composite command | Monthly summaries exportable as CSV and Obsidian markdown |
| 6 | Polish + Docker + CI | Docker deployment; CI green; app ready for daily use |

## Open Questions (from `docs/fynance-project-note.md`)

These have not been resolved and affect specific implementation choices. Decisions should be made before the relevant phase:

1. **Budgets: standing vs per-month?** (affects Phase 3) -- current schema is per-month. Standing budgets are simpler but do not support seasonal variation.
2. **HoldingType::Cash** (affects Phase 4) -- should uninvested cash in brokerage accounts be a `holdings` row or only reflected in the account balance?
3. **Holdings vs portfolio_snapshots consolidation** (affects Phase 4) -- Nonso and Ope both flagged overlap. Decide before building Phase 4 storage layer.
4. **ingestion_checklist: derive from import_log instead of a dedicated table?** (affects Phase 3) -- could simplify the schema.
5. **Docker for MVP: needed now?** (affects Phase 6) -- could defer to post-MVP to keep Phase 6 focused on polish.
