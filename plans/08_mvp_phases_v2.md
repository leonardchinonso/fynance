# MVP Implementation Phases v2

This supersedes `07_phases.md`. The architecture has changed: Obsidian integration is dropped in favor of a purpose-built local web UI (Axum + React). See `../design/` for the full design rationale.

## Overview

| Phase | Goal | Deliverable |
|---|---|---|
| 1 | Rust project scaffold + CSV import + SQLite | `fynance import` works, data in DB |
| 2 | Axum API server + embedded React shell | Browser opens, blank UI served |
| 3 | Transactions view + Budget tab | Core UI working with real data |
| 4 | Portfolio overview + account management | Net worth view live |
| 5 | Categorization pipeline | 90%+ transactions categorized |
| 6 | Reports + polish | Monthly summaries, export, UX refinement |

---

## Phase 1: Core Data Layer (Week 1)

**Goal**: A working Rust binary that reads CSV bank statements and stores transactions in SQLite. No UI, no AI, no async.

### Files to create

1. `Cargo.toml` — Phase 1 dependencies only
2. `sql/schema.sql` — full schema (transactions, accounts, import_log, budgets, portfolio_snapshots)
3. `src/model.rs` — Transaction, Account, AccountType, CategorySource
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

### Rust side

1. Add Axum + Tokio to `Cargo.toml`
2. Create `src/server/mod.rs` — Axum router, CORS, static file handler
3. Create `src/server/static_files.rs` — `include_dir!` embedding of `frontend/dist/`
4. Create `src/commands/serve.rs` — bind server, open browser
5. Extend `src/cli.rs` with `serve` subcommand

```toml
# Add to Cargo.toml
axum = "0.7"
tokio = { version = "1", features = ["full"] }
tower-http = { version = "0.5", features = ["cors", "fs"] }
include_dir = "0.7"
open = "5"                  # cross-platform browser open
```

### Frontend side

```bash
cd frontend
npm create vite@latest . -- --template react-ts
npm install recharts @radix-ui/react-slot class-variance-authority clsx tailwind-merge
npm install -D tailwindcss postcss autoprefixer @types/recharts
npx tailwindcss init -p
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

### Deliverable

```bash
fynance serve
# fynance: server started at http://localhost:3000
# (browser opens automatically)
```

Browser shows a nav bar with four tabs. All tabs show "Coming soon."

---

## Phase 3: Transactions View + Budget Tab (Week 3)

**Goal**: Real data visible in the browser. Users can filter transactions and see budget vs actual.

### Axum routes to implement

- `GET /api/transactions` — paginated, filterable by month/category/account
- `GET /api/transactions/categories` — list of distinct categories
- `GET /api/transactions/accounts` — list of accounts with transaction counts
- `GET /api/budget/:month` — budget vs actual per category for a month
- `POST /api/budget` — set a budget amount for a category+month
- `GET /api/monthly-income/:month` — income for a month
- `POST /api/monthly-income` — set income

### React Transactions page

- A date range picker (default: current month)
- Account selector (multi-select)
- Category selector (multi-select)
- Table: date, merchant, category, amount, confidence badge
- Pagination controls
- Total spending for the filtered view

### React Budget page

- Month picker
- Income bar at top (budgeted vs actual income)
- Category rows showing:
  - Progress bar: actual / budgeted
  - Amounts: `£278 / £300`
  - Color: green (<80%), amber (80-100%), red (>100%)
- "Edit budget" mode to set amounts per category
- Stacked bar chart: spending by category over last 6 months

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

**Goal**: Net worth view with account balances, diversity charts, and net worth trend.

### New CLI commands

```bash
fynance account add --id monzo-current --name "Monzo Current" \
    --institution Monzo --type checking --balance 1240.00

fynance account set-balance monzo-current 1240.00 --date 2026-04-10

fynance account list
```

### Axum routes

- `GET /api/portfolio` — full portfolio snapshot (see design/04_portfolio_overview.md)
- `POST /api/accounts` — register account
- `PATCH /api/accounts/:id/balance` — update balance
- `GET /api/portfolio/history` — monthly net worth snapshots

### React Portfolio page

- **Net Worth card**: headline figure + month-over-month delta
- **Accounts grid**: card per account with balance, type badge, "Update" button
- **Diversity donut charts**: by type, by institution (Recharts `PieChart`)
- **Net Worth line chart**: 12-24 month trend (Recharts `LineChart`)
- **Cash Flow bar chart**: income vs spending per month (Recharts `BarChart`)

### Deliverable

Portfolio tab shows net worth, all account balances, and diversity breakdown charts.

---

## Phase 5: Categorization Pipeline (Week 5)

**Goal**: 90%+ of transactions categorized.

### Work items

1. `config/categories.yaml` — full taxonomy
2. `config/rules.yaml` — patterns for Monzo/Revolut/Lloyds merchant names
3. `src/categorizer/rules.rs` — YAML rule loader + match_rules()
4. `src/categorizer/claude.rs` — Claude Haiku API with prompt caching
5. `src/categorizer/pipeline.rs` — rule-first, then Claude for unknowns
6. CLI: `fynance categorize [--batch]`
7. Axum: `POST /api/categorize` to trigger from UI
8. React: "Run categorization" button with progress indicator
9. Inline category editing in Transactions table (click to change)

### Deliverable

```bash
fynance categorize
# Rules: 1,640 matched (89%)
# Claude: 182 sent, 180 returned
# Total categorized: 1,820 / 1,842 (99%)
# Remaining: 22 — run `fynance review`
```

---

## Phase 6: Reports + Polish (Week 6+)

**Goal**: Exportable summaries, UI refinement, smooth monthly workflow.

### Work items

- `GET /api/reports/:month` — Claude-generated monthly summary
- `GET /api/export?year=2025&format=csv` — filtered CSV export
- React Reports page: monthly narrative + key stats
- `fynance monthly` composite command: import + categorize + snapshot
- Dark mode toggle (Tailwind `dark:` classes)
- Mobile-responsive layout (Tailwind responsive prefixes)
- Transaction notes field (click to annotate)
- `--dry-run` flag on import

---

## Build Workflow

```bash
# Build everything (frontend first, then embed in Rust binary)
make build
# which runs:
#   cd frontend && npm run build
#   cargo build --release

# Development (separate processes for hot-reload)
make dev-backend   # cargo run -- serve --no-open
make dev-frontend  # cd frontend && npm run dev -- --proxy http://localhost:3000
```

The dev frontend proxies `/api/*` to the Rust server. Production serves the embedded Vite bundle.

---

## Decisions Superseded by This Plan

| Old plan | New plan | Reason |
|---|---|---|
| Obsidian as UI | React web app | Proper UI needed; portfolio view impossible in Obsidian |
| SQLite DB plugin for queries | Axum REST API | More flexible, works without Obsidian |
| `fynance report` generates .md notes | Reports tab in browser | Single surface for all views |
| CLI-only interaction | CLI + browser UI | Requirement from Prompt 1.1 |
| No portfolio tracking | Portfolio tab | New requirement |
