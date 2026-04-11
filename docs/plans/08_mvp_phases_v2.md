# MVP Implementation Phases v2

This supersedes `07_phases.md`. The architecture has changed: Obsidian integration is dropped in favor of a purpose-built local web UI (Axum + React). See `../design/` for the full design rationale.

## Overview

| Phase | Goal | Deliverable |
|---|---|---|
| 1 | Rust project scaffold + CSV import + SQLite | `fynance import` works, data in DB |
| 2 | Axum API server + embedded React shell + API tokens | Browser opens, blank UI served, programmatic API ready |
| 3 | Transactions view + Budget tab | Core UI working with real data |
| 4 | Portfolio overview + account management | Net worth view live |
| 5 | API-first categorization | Agent-readable API docs, external categorization workflow |
| 6 | Reports + polish | Monthly summaries, export, Obsidian export |

---

## Phase 1: Core Data Layer (Week 1)

**Goal**: A working Rust binary that reads CSV bank statements and stores transactions in SQLite. No UI, no AI, no async.

### Files to create

1. `README.md` — project overview, prerequisites, dev setup, Docker deployment, CLI usage
2. `Cargo.toml` — Phase 1 dependencies only
3. `sql/schema.sql` — full schema (transactions, accounts, import_log, budgets, portfolio_snapshots)
3. `src/model.rs` — Transaction, Account, AccountType, CategorySource (all types derive `ts_rs::TS` for auto-generated TypeScript bindings)
4. `src/util.rs` — normalize_description, fingerprint, parse_date
5. `src/storage/db.rs` — Db::open(), insert_transaction(), log_import(), get_accounts()
6. `src/importers/mod.rs` — Importer trait
7. `src/importers/csv_importer.rs` — generic CSV + Monzo, Revolut, Lloyds mappings
8. `src/commands/import.rs` — parse file, insert, print summary
9. `src/commands/stats.rs` — transaction count, date range, per-account breakdown
10. `src/cli.rs` + `src/main.rs`

### Phase 1 Cargo.toml dependencies

```toml
[dependencies]
clap = { version = "4", features = ["derive"] }
rusqlite = { version = "0.31", features = ["bundled"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
csv = "1"
chrono = { version = "0.4", features = ["serde"] }
rust_decimal = { version = "1", features = ["serde-with-str"] }
uuid = { version = "1", features = ["v4"] }
anyhow = "1"
thiserror = "1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
once_cell = "1"
regex = "1"
sha2 = "0.10"
hex = "0.4"
indicatif = "0.17"
dirs = "5"
ts-rs = { version = "10", features = ["serde-compat", "chrono-impl"] }

[dev-dependencies]
tempfile = "3"
pretty_assertions = "1"
```

### Bank CSV Formats

#### Monzo
```
Transaction ID,Date,Time,Type,Name,Emoji,Category,Amount,Currency,Local amount,Local currency,Notes and #tags,Address,Receipt,Description,Category split,Money Out,Money In
tx_00009,...,2026-03-10,12:30:00,Payment,Lidl,,Groceries,-5.50,GBP,...
```
Key columns: `Date` (YYYY-MM-DD), `Amount` (negative debit), `Name` (merchant), `Transaction ID` (fitid), `Category` (optional)

#### Revolut
```
Type,Product,Started Date,Completed Date,Description,Amount,Fee,Currency,State,Balance
CARD_PAYMENT,Current,2026-03-10 12:30:00,...,Lidl,-5.5,0,GBP,COMPLETED,1234.5
```
Key columns: `Completed Date`, `Amount` (negative debit), `Description`

#### Lloyds Bank
```
Transaction Date,Transaction Type,Sort Code,Account Number,Transaction Description,Debit Amount,Credit Amount,Balance
10/03/2026,DEB,'11-22-33,12345678,LIDL GB LONDON,5.50,,1000.00
```
Key columns: `Transaction Date` (DD/MM/YYYY), `Transaction Description`, `Debit Amount`, `Credit Amount`

### Deliverable

```bash
cargo run -- import monzo_march.csv --account monzo-current
# Imported 142 new, 0 duplicates in 0.3s

cargo run -- stats
# Total: 1,842 transactions (2024-01-01 – 2026-04-10)
# monzo-current: 1,120 | revolut-main: 722
```

---

## Phase 2: Axum Server + React Shell (Week 2)

**Goal**: A running local web server that serves a React app in the browser. No real data in the UI yet — just the shell with navigation working.

### Single port for everything

The Axum server exposes one HTTP port (`FYNANCE_PORT`, default 7433) that serves both the web UI and all REST API endpoints (`/api/import`, `/api/import/csv`, `/api/import/bulk`, `/api/import/screenshot`, `/api/categorize`, etc.). In Docker, this single port is mapped to the host. Scripts, agents, and the browser all hit the same URL and port. No separate API port is needed.

### Rust side

1. Add Axum + Tokio to `Cargo.toml`
2. Create `src/server/mod.rs` — Axum router, CORS, static file handler
3. Create `src/server/static_files.rs` — `include_dir!` embedding of `frontend/dist/`
4. Create `src/commands/serve.rs` — bind server, open browser
5. Extend `src/cli.rs` with `serve` subcommand

### TypeScript bindings (ts-rs)

All Rust types in `src/model.rs` derive `ts_rs::TS` with `#[ts(export)]`. Running `cargo test` auto-generates TypeScript interfaces into `frontend/src/bindings/`:

```
frontend/src/bindings/
  Transaction.ts
  Account.ts
  AccountType.ts
  Budget.ts
  Holding.ts
  PortfolioSnapshot.ts
  ...
```

The frontend imports these directly: `import type { Transaction } from '../bindings/Transaction'`. Types are always in sync with the Rust backend. If a field is added or changed in Rust, TypeScript compilation will catch any frontend mismatches.

For Phase 2, the frontend uses **mock data** typed against these generated interfaces. This lets us validate the UI and interactions before wiring up real API calls.

```toml
# Add to Cargo.toml
axum = "0.7"
tokio = { version = "1", features = ["full"] }
tower-http = { version = "0.5", features = ["cors", "fs"] }
include_dir = "0.7"
open = "5"                  # cross-platform browser open
```

### Frontend side

React 19 with the React Compiler (automatic memoization at build time, no manual `useMemo`/`useCallback`/`React.memo` needed).

```bash
cd frontend
npm create vite@latest . -- --template react-ts
# Core: React 19
npm install react@19 react-dom@19
npm install recharts @radix-ui/react-slot class-variance-authority clsx tailwind-merge
# Dev: React Compiler + Tailwind
npm install -D babel-plugin-react-compiler @types/react@19 @types/react-dom@19
npm install -D tailwindcss postcss autoprefixer @types/recharts
npx tailwindcss init -p
```

Enable the React Compiler in `vite.config.ts`:
```ts
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [
    react({
      babel: {
        plugins: ["babel-plugin-react-compiler"],
      },
    }),
  ],
});
```

Frontend structure:
```
frontend/src/
  App.tsx           # Router with [Transactions, Budget, Portfolio, Reports] tabs
  pages/
    Transactions.tsx  # Placeholder
    Budget.tsx        # Placeholder
    Portfolio.tsx     # Placeholder
    Reports.tsx       # Placeholder
  components/
    Navbar.tsx
    LoadingSpinner.tsx
  api/
    client.ts         # fetch('/api/...') wrappers with error handling
```

Build script in `Cargo.toml` or a `Makefile`:
```bash
cd frontend && npm run build   # outputs to frontend/dist/
cargo build --release          # embeds frontend/dist/ via include_dir!
```

### API Token Authentication

The REST API supports token-based auth for programmatic access (scripts, agents, automation). Tokens are generated locally and stored hashed in the DB.

```bash
# Generate a new API token
fynance token create --name "import-script"
# Token: fyn_a1b2c3d4e5f6...  (shown once, store it securely)

# List active tokens
fynance token list

# Revoke a token
fynance token revoke --name "import-script"
```

Schema addition:
```sql
CREATE TABLE IF NOT EXISTS api_tokens (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT NOT NULL UNIQUE,
    token_hash  TEXT NOT NULL,          -- SHA-256 of the token
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    last_used   TEXT,
    is_active   INTEGER NOT NULL DEFAULT 1
);
```

Token usage:
```bash
# Bulk import via API (agent or script)
curl -X POST http://localhost:7433/api/import/csv \
  -H "Authorization: Bearer fyn_a1b2c3d4e5f6..." \
  -F "file=@monzo_march.csv" \
  -F "account=monzo-current"

# Multiple files
curl -X POST http://localhost:7433/api/import/bulk \
  -H "Authorization: Bearer fyn_a1b2c3d4e5f6..." \
  -F "files=@monzo_march.csv" \
  -F "files=@revolut_march.csv" \
  -F "accounts=monzo-current,revolut-main"
```

Browser UI requests from localhost skip token auth (same loopback trust model as before). Tokens are only required for programmatic API access. This lets agents and scripts push data without exposing the full app.

### Typed JSON Import API

In addition to CSV upload, a typed JSON endpoint (`POST /api/import`) accepts structured transaction data. This is the opinionated import path for AI agents and external tools that extract data from unsupported sources, convert it to the fynance schema, and push it via the API.

```bash
curl -X POST http://localhost:7433/api/import \
  -H "Authorization: Bearer fyn_a1b2c3d4e5f6..." \
  -H "Content-Type: application/json" \
  -d '{
    "account_id": "monzo-current",
    "transactions": [
      {
        "date": "2026-03-10",
        "description": "LIDL GB LONDON",
        "amount": "-5.50",
        "currency": "GBP"
      }
    ]
  }'
```

The JSON schema is documented in the OpenAPI spec at `GET /api/docs`. This enables a workflow where an external agent (e.g., a Claude Code script) reads data from an unsupported bank, extracts transactions into the typed format, and hits this endpoint directly.

### API Documentation (Agent-Readable)

The REST API is documented via a `GET /api/docs` endpoint. This is the primary interface for external AI agents, so the docs are designed to be usable as a system prompt:

- **OpenAPI/Swagger JSON spec** with full request/response schemas, field descriptions, and example payloads
- **Category taxonomy** included in the docs so agents know the valid categories and hierarchy
- **Import schema** with explicit field definitions, required vs optional fields, and validation rules
- **Categorization guidance**: the docs explain that agents should categorize transactions before pushing them (rule-based or AI), and include the `category_source` field (`'agent'`, `'manual'`, `'rule'`) so the app can track provenance
- **Error responses** documented so agents can handle failures gracefully

The goal is that an AI agent can fetch `GET /api/docs`, use the response as context, and immediately start interacting with the API without any additional documentation. Think of it as a CLAUDE.md for the API.

### Deliverable

```bash
fynance serve
# fynance: server started at http://localhost:7433
# (browser opens automatically)
```

Browser shows a nav bar with four tabs. All tabs show "Coming soon." API token generation works via CLI.

---

## Phase 3: Transactions View + Budget Tab (Week 3)

**Goal**: Real data visible in the browser. Users can filter transactions and see budget vs actual.

### Axum routes to implement

- `GET /api/transactions` — paginated, filterable by month/category/account
- `GET /api/transactions/categories` — list of distinct categories
- `GET /api/transactions/accounts` — list of accounts with transaction counts
- `GET /api/budget/:month` — budget vs actual per category for a month
- `POST /api/budget` — set a budget amount for a category+month
- `GET /api/income/:month` — derived from transactions in the Income category for that month (no separate table)

### React Transactions page

- **Date range selector** (always visible at top): presets for current month, last 3 months, YTD, full year, last 5 years, custom range. Zoom in/out and pan through time. All views update reactively.
- Account selector (multi-select toggle)
- Category selector (multi-select toggle)
- **Multiple view modes** for the same data:
  - **Table**: date, merchant, category, amount, confidence badge, pagination, total spending
  - **Bar chart**: spending over time, grouped by category
  - **Pie chart**: interactive, hover for tooltips showing percentage and amount (e.g., "Feeding: 23%, £3,500")
- Export any view as image, CSV, or markdown

### React Budget page

- **Date range selector** at top (same universal component)
- Income bar at top (budgeted vs actual income)
- Category rows showing:
  - Progress bar: actual / budgeted
  - Amounts: `£278 / £300`
  - Color: **red** for over budget (>110%), **amber** for near budget (80-110%), **green** for under budget (<80%). Thresholds are configurable in the settings page (default: green < 80%, amber 80-110%, red > 110%)
- "Edit budget" mode to set amounts per category
- **Multiple view modes**:
  - **Table**: spreadsheet-style, spending per category per month with color coding
  - **Stacked bar chart**: spending by category over the selected time range
  - **Line chart**: spending trends per category over time
  - **Pie chart**: category breakdown for the selected period, interactive with tooltips
- **Budget planning**: view historical spending per category across many months to inform future budget decisions (e.g., "What have I spent on food over the last 12 months on average?")
- Export any view

### Guided Monthly Ingestion Flow

- `GET /api/ingestion/checklist/:month` — returns all active accounts with their update status for a given month
- `POST /api/ingestion/checklist/:month/:account_id` — mark an account as completed/skipped
- React component: a step-by-step wizard shown during monthly review
  - Lists all configured accounts with status badges (pending/completed/skipped)
  - Progress indicator: "3 of 7 accounts updated for March 2026"
  - Each step prompts the user to upload CSV, enter balance, or skip
  - Completing all steps marks the month as fully reviewed

### Storage additions

```sql
-- Needed for budget queries
SELECT
    category,
    SUM(ABS(CAST(amount AS REAL))) AS actual,
    b.amount AS budgeted
FROM transactions t
LEFT JOIN budgets b
    ON b.month = substr(t.date, 1, 7)
    AND b.category = t.category
WHERE substr(t.date, 1, 7) = ?1
    AND CAST(amount AS REAL) < 0
GROUP BY t.category
ORDER BY actual DESC;
```

### Deliverable

Full transaction list with filtering. Budget tab shows real spending per category. User can set budget amounts.

---

## Phase 4: Portfolio Overview (Week 4)

**Goal**: Net worth view with account balances, stock-level holdings, diversity charts, net worth trend, and filtering.

### New CLI commands

```bash
fynance account add --id monzo-current --name "Monzo Current" \
    --institution Monzo --type checking --balance 1240.00

fynance account set-balance monzo-current 1240.00 --date 2026-04-10

fynance account list
```

### Axum routes

- `GET /api/portfolio` — full portfolio snapshot with carry-forward semantics (see design/04_portfolio_overview.md)
- `GET /api/portfolio?as_of=2026-01-31` — portfolio as of a specific date (uses last known values)
- `POST /api/accounts` — register account
- `PATCH /api/accounts/:id/balance` — update balance
- `GET /api/portfolio/history` — monthly net worth snapshots
- `GET /api/holdings/:account_id` — stock-level holdings for an investment account
- `GET /api/holdings/:account_id/:symbol` — detail for a single holding (including ETF composition if available)
- `POST /api/holdings/:account_id` — bulk update holdings from platform export

### React Portfolio page

- **Date range selector** at top (same universal component)
- **Net Worth card**: headline figure + delta from previous period
- **Accounts grid**: card per account with balance, type badge, "Update" button, staleness indicator ("as of Jan 2023" if data is carried forward)
- **Filter toggles**: check/uncheck accounts, account types, or institutions to include/exclude from all charts (e.g., "hide pension", "show only liquid assets")
- **Multiple view modes**:
  - **Table**: all accounts with balances, sortable
  - **Net Worth line chart**: over the selected time range (Recharts `LineChart`)
  - **Allocation pie/donut charts**: by type, by institution (Recharts `PieChart`), interactive with tooltips
  - **Cash Flow bar chart**: income vs spending per month (Recharts `BarChart`)
  - **Stacked area chart**: account balance composition over time
- **Stock-level drill-down**: click an investment account to see individual holdings (stocks, ETFs, funds) with quantities, values, and allocation percentages
- **ETF composition (V1+)**: opt-in checkbox "Include individual stocks within ETFs". Only when checked does the app fetch ETF breakdown on demand from a free API and cache it. This feature is only available for **current holdings** (no historical ETF composition). Without the checkbox, ETFs display as a single line item (e.g., "VWRL: £8,000")
- Export any view

### Point-in-time carry-forward

When querying portfolio data for a date where no update was recorded, the system returns the most recent prior value. The frontend displays a staleness indicator so the user knows the data may be outdated. See `design/03_data_model.md` Key Decision #5 for the query pattern.

### Deliverable

Portfolio tab shows net worth, all account balances with staleness indicators, stock-level holdings for investment accounts, diversity breakdown charts, and full filtering/toggle support.

---

## Phase 5: Categorization Pipeline (Week 5)

**Goal**: 90%+ of transactions categorized via external AI agents pushing pre-categorized data through the API.

<!-- DEFERRED: Internal categorization pipeline (rules + Claude API). For MVP, categorization
happens externally. An AI agent reads the API docs, fetches uncategorized transactions, applies
rules and/or AI categorization, and pushes results back via PATCH /api/transactions/:id.

Original internal work items:
1. config/categories.yaml — full taxonomy
2. config/rules.yaml — patterns for Monzo/Revolut/Lloyds merchant names
3. src/categorizer/rules.rs — YAML rule loader + match_rules()
4. src/categorizer/claude.rs — Claude Haiku API with prompt caching
5. src/categorizer/pipeline.rs — rule-first, then Claude for unknowns
6. CLI: fynance categorize [--batch]
7. Axum: POST /api/categorize to trigger from UI
8. React: "Run categorization" button with progress indicator
-->

### MVP approach: API-first categorization

For MVP, categorization is not built into the binary. Instead:

1. `config/categories.yaml` -- full taxonomy (used by both the UI and external agents)
2. The API docs at `GET /api/docs` include the category taxonomy, field schemas, and example payloads. The docs are written to be usable as a system prompt for AI agents (like a CLAUDE.md for the API).
3. External agents (Claude Code scripts, MCP tools, etc.) read the API docs, fetch uncategorized transactions via `GET /api/transactions?category=uncategorized`, apply their own categorization logic (rules, AI, or both), and push results via `PATCH /api/transactions/:id` with `category` and `category_source = 'agent'`.
4. The `POST /api/import` typed JSON endpoint accepts pre-categorized transactions: agents can extract data from unsupported sources, categorize it, and push it in one step.
5. Inline category editing in Transactions table (click to change) -- manual override in the UI.

This model means the fynance binary has zero outbound API calls. All AI processing happens outside, and the app is a clean data store + UI.

### Deliverable

External agent workflow:
```bash
# Agent reads API docs, fetches uncategorized transactions, categorizes them, pushes back
# Result: 90%+ of transactions categorized without any internal AI code
```

Manual fallback: user clicks a transaction in the UI and assigns a category.

---

## Phase 6: Reports + Polish (Week 6+)

**Goal**: Exportable summaries, UI refinement, smooth monthly workflow, and low-friction data ingestion.

### Work items

<!-- DEFERRED: - `GET /api/reports/:month` — Claude-generated monthly summary (internal AI, deferred) -->
- `GET /api/reports/:month` -- data-driven monthly summary (totals, category breakdown, budget status; no AI narrative for MVP)
- `GET /api/export?year=2025&format=csv` -- filtered CSV export
- `GET /api/export?year=2025&format=md` -- Obsidian-compatible markdown export (see below)
- React Reports page: monthly stats + charts
- `fynance monthly` composite command: import + snapshot (no internal categorize step for MVP)
- Dark mode toggle (Tailwind `dark:` classes)
- Mobile-responsive layout (Tailwind responsive prefixes)
- Transaction notes field (click to annotate)
- `--dry-run` flag on import

<!-- DEFERRED: Screenshot Data Extraction (internal Claude Vision)

An endpoint that accepts images and uses Claude Vision (Sonnet) to extract transaction data.
POST /api/import/screenshot. This requires ANTHROPIC_API_KEY and internal AI.

For MVP, screenshot extraction is handled externally: an AI agent reads a screenshot,
extracts transactions, and pushes them through POST /api/import as typed JSON.
The API docs describe the expected schema so agents can do this without internal support.
-->

### Obsidian Markdown Export

Export monthly summaries as markdown files compatible with Obsidian vaults.

```bash
fynance export --month 2026-03 --format md --output ~/SecondBrain/03-finance/
# Creates: 2026-03-monthly-summary.md
```

API:
```
GET /api/export?month=2026-03&format=md
```

Output format:
```markdown
# March 2026 Financial Summary

## Overview
- Total spending: £2,940.18
- Total income: £4,500.00
- Net: +£1,559.82

## Spending by Category
| Category | Amount | Budget | % Used |
|---|---|---|---|
| Food: Groceries | £278.42 | £300.00 | 92.8% |
| ...

## Net Worth
£28,450.00 (+£320 from last month)

## Notes
(User-written notes or externally generated narrative)
```

The markdown format uses standard tables and headings so it renders well in Obsidian, GitHub, and any markdown viewer.

---

## Future Roadmap (Post-MVP)

These features are out of scope for the initial MVP but are planned for future iterations. Documenting them now so architectural decisions don't paint us into a corner.

### V1: Internal AI Workflows (Optional)

When/if we want the binary to handle AI internally rather than relying on external agents:

- `ANTHROPIC_API_KEY` support in `.env`
- `src/categorizer/claude.rs` -- Claude Haiku API with prompt caching for categorization
- `src/categorizer/pipeline.rs` -- rule-first, then Claude for unknowns
- `POST /api/categorize` -- trigger internal categorization from the UI
- `POST /api/import/screenshot` -- Claude Vision (Sonnet) to extract transactions from images
- `GET /api/reports/:month` -- Claude-generated narrative monthly summaries
- CLI: `fynance categorize [--batch]`

This is optional and additive. The external agent model remains the primary path. Internal AI is a convenience layer for users who want one-click categorization without running a separate agent.

### V1: Stock Price Service

Live stock price lookups for inter-month portfolio accuracy. Architecture:

- **Price source**: free API (e.g., Yahoo Finance). Must be free, no paid tiers.
- **Storage**: `stock_prices` table in SQLite: `(symbol, date, close_price, currency)`
- **Cadence**: configurable in settings, default monthly sampling. User can increase to weekly/daily.
- **On-demand loading**: when the frontend requests a visualization (e.g., yearly net worth chart), the backend checks which prices are needed, fetches any missing ones from the API, stores them, and returns the data. The frontend just requests "prices for symbols X,Y,Z between date A and date B at monthly frequency" and the backend handles the rest.
- **Size-based LRU cache**: to prevent database bloat, set a configurable max size for the stock_prices table (e.g., 50MB). When the limit is reached, drop the oldest prices first. Old prices can always be re-fetched on demand if a user scrolls back to that time period.
- **Rate limiting**: batch requests, respect API rate limits, never fetch the same (symbol, date) twice.

```sql
CREATE TABLE IF NOT EXISTS stock_prices (
    symbol      TEXT NOT NULL,
    date        TEXT NOT NULL,             -- YYYY-MM-DD
    close_price TEXT NOT NULL,             -- Decimal string
    currency    TEXT NOT NULL DEFAULT 'GBP',
    fetched_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    PRIMARY KEY (symbol, date)
);
```

### V1: ETF Composition (On-Demand)

- Opt-in checkbox in portfolio view: "Include individual stocks within ETFs"
- Only fetches composition when the user explicitly requests it
- Only available for **current holdings** (no historical ETF breakdown, compositions change quarterly)
- Cached in a `etf_compositions` table with a TTL (refresh quarterly)
- Source: free API (e.g., ETF breakdown data from public sources)

### V1: Settings Page

A settings page for advanced configuration:
- Budget color thresholds (green/amber/red boundaries, default: 80%/110%)
- Stock price fetch frequency (monthly/weekly/daily)
- Stock price cache size limit
- Default date range for visualizations
- Default view mode (table/chart/pie)
- Account ordering/grouping preferences

### V1+: Forecasting & Planning

1. **Forecasting**: based on historical spending patterns, project future spending, income, and net worth. "If I continue at this rate, what does my net worth look like in 6/12/24 months?"
2. **Big purchase planning**: set a savings target and date, track progress, project when you will hit it based on current savings rate.
3. **Early retirement modeling**: starting balance, spending rate, investment returns, project account balance over time.

### V2+: Tax & Rental

4. **Tax planning for capital gains** — track buy/sell of investments, compute CGT liability, surface allowance usage.
5. **Rental income tracking** — income and expense tracking for rental properties, useful for self-assessment.

### V2+: AI Chat

6. **AI chat interface** — conversational window to ask questions about your finances or dump screenshots for extraction. Uses Claude with access to your transaction data.

These will be specced and phased when the MVP is stable.

---

## Configuration (.env)

All configurable values are read from environment variables, with sensible defaults for local development. A `.env` file at the project root is loaded automatically (via `dotenvy` in Rust, Vite's built-in `.env` support for the frontend).

```env
# .env.example
FYNANCE_PORT=7433                          # HTTP server port
FYNANCE_DB_PATH=                           # SQLite path (default: OS data dir)
FYNANCE_HOST=127.0.0.1                     # Bind address (use 0.0.0.0 in Docker)
ANTHROPIC_API_KEY=                          # Claude API key (optional)
FYNANCE_LOG_LEVEL=info                     # tracing filter
```

In Rust, add `dotenvy = "0.15"` to `Cargo.toml` and call `dotenvy::dotenv().ok()` early in `main()`. Each config value falls back to a sensible default if the env var is unset:

| Variable | Default | Notes |
|---|---|---|
| `FYNANCE_PORT` | `7433` | |
| `FYNANCE_DB_PATH` | `dirs::data_local_dir()/fynance/fynance.db` | Override for Docker volume mount |
| `FYNANCE_HOST` | `127.0.0.1` | Set to `0.0.0.0` inside Docker so the port mapping works |
| `ANTHROPIC_API_KEY` | (none) | Optional, categorization degrades gracefully without it |
| `FYNANCE_LOG_LEVEL` | `info` | Passed to `tracing_subscriber::EnvFilter` |

---

## Development Workflow

Two terminal processes for hot-reload development:

### Backend (Rust)

Use `cargo-watch` for live rebuild on file changes:

```bash
# Install once
cargo install cargo-watch

# Run with live reload (rebuilds + restarts on any .rs change)
cargo watch -x 'run -- serve --no-open'
```

This restarts the Axum server whenever Rust source files change. The `--no-open` flag prevents the browser from reopening on each restart.

### Frontend (Vite)

Vite dev server with HMR (hot module replacement), proxying API calls to the Rust backend:

```bash
cd frontend
npm run dev
```

Vite config includes a proxy so `/api/*` requests go to the Rust server:

```ts
// vite.config.ts (add to the defineConfig)
server: {
  port: 5173,
  proxy: {
    "/api": {
      target: "http://localhost:7433",
      changeOrigin: true,
    },
  },
},
```

### Makefile targets

```makefile
dev-backend:
	cargo watch -x 'run -- serve --no-open'

dev-frontend:
	cd frontend && npm run dev

dev:
	# Run both in parallel (requires a process manager or two terminals)
	@echo "Run 'make dev-backend' and 'make dev-frontend' in separate terminals"

build:
	cd frontend && npm run build
	cargo build --release
```

---

## Production Deployment (Docker)

Single-container Docker image: multi-stage build compiles both the frontend and the Rust binary, producing a minimal final image. SQLite data persists on a Docker volume.

### Dockerfile

```dockerfile
# Stage 1: Build frontend
FROM node:22-alpine AS frontend-build
WORKDIR /app/frontend
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci
COPY frontend/ ./
RUN npm run build

# Stage 2: Build Rust binary
FROM rust:1.85-slim AS rust-build
WORKDIR /app
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
COPY sql/ sql/
COPY config/ config/
COPY --from=frontend-build /app/frontend/dist frontend/dist/
RUN cargo build --release

# Stage 3: Minimal runtime
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
RUN useradd -m -s /bin/bash fynance

COPY --from=rust-build /app/target/release/fynance /usr/local/bin/fynance

USER fynance
WORKDIR /home/fynance

# SQLite data lives here, mount as a volume
RUN mkdir -p /home/fynance/data

ENV FYNANCE_HOST=0.0.0.0
ENV FYNANCE_PORT=7433
ENV FYNANCE_DB_PATH=/home/fynance/data/fynance.db

EXPOSE 7433

CMD ["fynance", "serve", "--no-open"]
```

### docker-compose.yml

For local builds:
```yaml
services:
  fynance:
    build: .
    ports:
      - "${FYNANCE_PORT:-7433}:7433"
    volumes:
      - fynance-data:/home/fynance/data
    env_file:
      - .env
    restart: unless-stopped

volumes:
  fynance-data:
```

For pulling from GHCR (production):
```yaml
services:
  fynance:
    image: ghcr.io/<owner>/fynance:latest    # or pin to a version: :v0.3.0
    ports:
      - "${FYNANCE_PORT:-7433}:7433"
    volumes:
      - fynance-data:/home/fynance/data
    env_file:
      - .env
    restart: unless-stopped

volumes:
  fynance-data:
```

### Usage

```bash
# First run (local build)
docker compose up -d

# First run (from GHCR, no local build needed)
docker compose pull && docker compose up -d

# Update to latest release
docker compose pull
docker compose up -d

# Rebuild locally after code changes
docker compose up -d --build

# View logs
docker compose logs -f fynance

# Stop
docker compose down

# Stop and delete data
docker compose down -v
```

The SQLite database persists in the `fynance-data` Docker volume. The `.env` file is passed through to the container, so `ANTHROPIC_API_KEY` and any overrides work the same way locally and in Docker.

### Bootstrap

The Rust binary handles DB bootstrapping automatically on startup:
1. `Db::open()` creates the SQLite file if it does not exist
2. Runs `sql/schema.sql` migrations
3. Creates default categories from `config/categories.yaml` if the categories table is empty

No separate bootstrap script needed. `docker compose up` is the single command to go from zero to running.

---

## CI/CD (GitHub Actions + GHCR)

All CI/CD runs on GitHub Actions (free tier: 2,000 minutes/month). Docker images are published to GitHub Container Registry (GHCR), which is free for public repos and has 500MB free for private repos.

### Workflow 1: CI on every push

`.github/workflows/ci.yml` — runs on every push and PR to `main`:

```yaml
name: CI
on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt -- --check
      - run: cargo clippy --all-targets -- -D warnings
      - run: cargo test

  frontend:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: 22
          cache: npm
          cache-dependency-path: frontend/package-lock.json
      - run: cd frontend && npm ci
      - run: cd frontend && npm run build
      - run: cd frontend && npm run lint
```

### Workflow 2: Build and push Docker image on push to main

`.github/workflows/docker-publish.yml` — builds and pushes to GHCR on every push to `main`, tagged as `latest`:

```yaml
name: Docker Publish
on:
  push:
    branches: [main]

env:
  REGISTRY: ghcr.io
  IMAGE_NAME: ${{ github.repository }}

jobs:
  build-and-push:
    runs-on: ubuntu-latest
    permissions:
      contents: read
      packages: write

    steps:
      - uses: actions/checkout@v4

      - uses: docker/login-action@v3
        with:
          registry: ${{ env.REGISTRY }}
          username: ${{ github.actor }}
          password: ${{ secrets.GITHUB_TOKEN }}

      - uses: docker/metadata-action@v5
        id: meta
        with:
          images: ${{ env.REGISTRY }}/${{ env.IMAGE_NAME }}
          tags: |
            type=raw,value=latest
            type=sha,prefix=

      - uses: docker/build-push-action@v6
        with:
          context: .
          push: true
          tags: ${{ steps.meta.outputs.tags }}
          labels: ${{ steps.meta.outputs.labels }}
```

### Workflow 3: Manual release with version tag

`.github/workflows/release.yml` — triggered manually from the GitHub Actions UI. You enter a version (e.g., `v0.3.0`), it builds, pushes the image with that version tag, and creates a GitHub Release:

```yaml
name: Release
on:
  workflow_dispatch:
    inputs:
      version:
        description: "Version tag (e.g., v0.3.0)"
        required: true
        type: string

env:
  REGISTRY: ghcr.io
  IMAGE_NAME: ${{ github.repository }}

jobs:
  release:
    runs-on: ubuntu-latest
    permissions:
      contents: write
      packages: write

    steps:
      - uses: actions/checkout@v4

      - uses: docker/login-action@v3
        with:
          registry: ${{ env.REGISTRY }}
          username: ${{ github.actor }}
          password: ${{ secrets.GITHUB_TOKEN }}

      - uses: docker/metadata-action@v5
        id: meta
        with:
          images: ${{ env.REGISTRY }}/${{ env.IMAGE_NAME }}
          tags: |
            type=raw,value=${{ inputs.version }}
            type=raw,value=latest

      - uses: docker/build-push-action@v6
        with:
          context: .
          push: true
          tags: ${{ steps.meta.outputs.tags }}
          labels: ${{ steps.meta.outputs.labels }}

      - uses: softprops/action-gh-release@v2
        with:
          tag_name: ${{ inputs.version }}
          name: ${{ inputs.version }}
          generate_release_notes: true
```

### Update flow for users

```bash
# Always get the latest build (auto-published on every push to main)
docker compose pull
docker compose up -d

# Pin to a specific release version
# Edit docker-compose.yml: image: ghcr.io/<owner>/fynance:v0.3.0
docker compose pull
docker compose up -d
```

No secrets to configure beyond the default `GITHUB_TOKEN` which is provided automatically. GHCR auth uses the same GitHub credentials.

---

## Decisions Superseded by This Plan

| Old plan | New plan | Reason |
|---|---|---|
| Obsidian as UI | React web app | Proper UI needed; portfolio view impossible in Obsidian |
| SQLite DB plugin for queries | Axum REST API | More flexible, works without Obsidian |
| `fynance report` generates .md notes | Reports tab in browser | Single surface for all views |
| CLI-only interaction | CLI + browser UI | Requirement from Prompt 1.1 |
| No portfolio tracking | Portfolio tab | New requirement |
| React 18 | React 19 + React Compiler | Starting fresh, no migration cost; auto-memoization removes boilerplate |
| No deployment story | Docker Compose single-container | One command to run in production; SQLite on a volume |
| Hardcoded paths/ports | `.env` configuration | Same config works locally, in Docker, and in CI |
| No programmatic API access | Token-authenticated REST API | Agents and scripts can push CSVs and screenshots directly |
| No image import | Screenshot extraction via Claude Vision | Low-friction ingestion from banking app screenshots |
| No Obsidian integration | Markdown export compatible with Obsidian | Monthly summaries exportable to Obsidian vaults |
| Manual Docker builds | GitHub Actions CI/CD + GHCR | Auto-publish on push to main; manual release with version tags; `docker compose pull` to update |
