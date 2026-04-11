# fynance — Claude Context

Before starting any work, read ~/SecondBrain/02-projects/fynance.md for the goal, decisions, and open questions behind this project.

## Overview

Personal finance tracker with a Rust backend and a local React web UI. Ingests bank CSV exports (Monzo, Revolut, Lloyds), categorizes transactions (rules + Claude API), stores everything in a per-user local SQLite database, and surfaces four views in the browser: Transactions, Budget, Portfolio, Reports.

Design documents live in `./design/`, research in `./research/`, implementation plans in `./plans/`. When picking up work, start at `plans/08_mvp_phases_v2.md` Phase 1. The older `plans/07_phases.md` is superseded.

## Architecture

Single Rust binary that:
1. Exposes a loopback-only Axum HTTP server (`127.0.0.1`) with a REST JSON API
2. Serves a compiled React frontend embedded via `include_dir!`
3. Stores data in a per-user SQLite database at the OS data directory
4. Optionally calls Claude API for categorization and monthly analysis

User runs `fynance serve`, the default browser opens, and all interaction happens in the browser. CLI subcommands remain available for scripting and automation.

## Tech Stack

- **Language**: Rust (edition 2024, MSRV 1.85)
- **CLI**: `clap` with derive macros
- **Web server**: `axum` on `tokio`, bound to `127.0.0.1` only
- **Storage**: SQLite via `rusqlite` at `~/.local/share/fynance/fynance.db` (macOS: `~/Library/Application Support/fynance/`)
- **Frontend**: React 18 + Vite + TypeScript + Tailwind + shadcn-ui + Recharts, embedded in the Rust binary via `include_dir!`
- **AI**: Claude API via `reqwest` + `serde_json` (Haiku for categorization, Sonnet for analysis)
- **Config**: YAML via `serde_yaml` for rules and categories; config file at `~/.config/fynance/config.yaml` mode `600`
- **Money**: `rust_decimal::Decimal`, never `f32` / `f64`

## Key Crates

| Crate | Purpose |
|---|---|
| `clap` | CLI argument parsing (derive) |
| `axum` | HTTP server |
| `tokio` | Async runtime |
| `tower-http` | CORS, static file middleware |
| `include_dir` | Embed compiled frontend bundle |
| `open` | Cross-platform browser launch |
| `rusqlite` | SQLite driver with bundled feature |
| `dirs` | Per-OS user data directory resolution |
| `reqwest` | HTTP client for Claude API |
| `serde`, `serde_json`, `serde_yaml` | Serialization and config |
| `csv` | CSV statement parsing |
| `regex` | Rule-based categorization patterns |
| `chrono` | Date handling |
| `rust_decimal` | Precise money math |
| `uuid` | Transaction IDs |
| `sha2`, `hex` | Fingerprint deduplication |
| `anyhow` | Application error context |
| `thiserror` | Library error enums |
| `tracing`, `tracing-subscriber` | Structured logging |
| `indicatif` | CLI progress bars during bulk imports |

## Commands

- Build backend: `cargo build --release`
- Build frontend: `cd frontend && npm run build`
- Build everything: `make build` (frontend first, then cargo)
- Run: `cargo run --release -- <subcommand>`
- Dev backend: `cargo run -- serve --no-open`
- Dev frontend: `cd frontend && npm run dev` (proxies `/api/*` to backend)
- Test: `cargo test`
- Lint: `cargo clippy --all-targets -- -D warnings`
- Format: `cargo fmt`
- Binary: `./target/release/fynance`

## CLI Subcommands

```bash
fynance serve [--port 3000] [--no-open]      # Start local web UI
fynance import <file|dir> --account <id>     # Import CSV statements
fynance categorize [--batch]                  # Run categorization pipeline
fynance account add --id <id> --name <name> --institution <inst> --type <type>
fynance account set-balance <id> <amount> --date YYYY-MM-DD
fynance account list
fynance budget set --month YYYY-MM --category <c> --amount N
fynance budget status
fynance stats
fynance export --year YYYY --format csv
fynance monthly                               # import + categorize + snapshot
```

## REST API Surface (served by `fynance serve`)

```
GET    /api/transactions?month=&category=&account=&page=&limit=
GET    /api/transactions/categories
GET    /api/transactions/accounts
PATCH  /api/transactions/:id                 # edit category, notes
POST   /api/import                           # upload CSV
POST   /api/categorize

GET    /api/budget/:month
POST   /api/budget
GET    /api/monthly-income/:month
POST   /api/monthly-income

GET    /api/portfolio
GET    /api/portfolio/history
POST   /api/accounts
PATCH  /api/accounts/:id/balance

GET    /api/reports/:month
GET    /api/export?year=&format=
```

## Security Model

This is a single-user local app. "Multi-user" means multiple OS users on the same machine, each running their own isolated instance.

- Axum binds to `127.0.0.1` only, never `0.0.0.0`. No LAN or internet exposure.
- Database path resolves from `dirs::data_local_dir()`, per OS user.
- Data directory created with mode `0o700`; DB file `0o600` on Unix.
- Claude API key read from `ANTHROPIC_API_KEY` env var or `~/.config/fynance/config.yaml` (mode `600`). Never logged, never stored in DB.
- Claude API receives only normalized merchant strings, never amounts, dates, or account IDs.
- No telemetry. Only outbound calls are explicit Claude API categorization.
- No auth for MVP. Loopback binding is the isolation boundary, same as local dev servers.

See `design/05_security_isolation.md` for details.

## Conventions

- Filenames use underscores, not spaces
- Avoid em dashes in any written content; use commas, colons, or separate sentences instead
- Money values use `rust_decimal::Decimal`, never `f32` or `f64`
- Dates stored as ISO 8601 strings (`YYYY-MM-DD`) in SQLite
- Money stored as TEXT in SQLite, parsed as `Decimal` in Rust; never store as REAL
- Return `anyhow::Result<T>` at command / route boundaries, define `thiserror` enums inside modules
- Every importer deduplicates by a stable fingerprint hash `sha256(date, amount, description, account_id)`
- Never log raw transaction descriptions at INFO level (may leak merchant info)
- Claude API calls always go through `src/categorizer/claude.rs` so prompt caching and batch behavior stay consistent
- Axum handlers return `Result<Json<T>, AppError>` where `AppError` implements `IntoResponse`
- Frontend fetches through `src/api/client.ts`, never direct `fetch()` in components
