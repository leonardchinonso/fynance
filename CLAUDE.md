# fynance — Claude Context

Before starting any work, read `docs/fynance-project-note.md` for project goals, the design docs in `docs/design/`, and the current plan at `docs/plans/08_mvp_phases_v2.md`.

## Repo

https://github.com/leonardchinonso/fynance

## Overview

Personal finance tracker with a Rust backend and a local React web UI. Ingests bank CSV exports (Monzo, Revolut, Lloyds), stores everything in a per-user local SQLite database, and surfaces four views in the browser: Transactions, Budget, Portfolio, Reports. Categorization and data extraction are handled by external AI agents that push pre-categorized data through the REST API.

Design documents live in `docs/design/`, research in `docs/research/`, implementation plans in `docs/plans/`. When picking up work, start at `docs/plans/08_mvp_phases_v2.md` Phase 1. The older `docs/plans/07_phases.md` is superseded.

## Architecture

Single Rust binary that:
1. Exposes a loopback-only Axum HTTP server (`127.0.0.1`) with a REST JSON API
2. Serves a compiled React frontend embedded via `include_dir!`
3. Stores data in a per-user SQLite database at the OS data directory
<!-- DEFERRED: 4. Optionally calls Claude API for categorization and monthly analysis -->
<!-- Internal AI workflows are deferred. For MVP, external AI agents handle categorization and data extraction, then push results through the REST API. -->

User runs `fynance serve`, the default browser opens, and all interaction happens in the browser. CLI subcommands remain available for scripting and automation.

## Repo Structure

```
fynance/
├── frontend/                # React 19 app (package.json, vite, etc.)
│   └── src/
│       └── bindings/        # auto-generated TypeScript types from Rust via ts-rs
├── backend/                 # Rust crate (Cargo.toml lives here)
│   ├── src/
│   └── config/              # categories.yaml, rules.yaml (TODO: may move to db/ as seed data)
├── db/                      # sql/schema.sql, migrations
├── assets/                  # shared assets (logo, etc.)
├── docs/                    # design docs, plans, research, prompts
│   ├── design/
│   ├── plans/
│   └── research/
├── .github/workflows/       # CI/CD
├── docker-compose.yml
├── Dockerfile
├── Makefile
├── .env.example
├── CLAUDE.md
└── README.md
```

## Tech Stack

- **Language**: Rust (edition 2024, MSRV 1.85)
- **CLI**: `clap` with derive macros
- **Web server**: `axum` on `tokio`, bound to `127.0.0.1` only
- **Storage**: SQLite via `rusqlite` at `~/.local/share/fynance/fynance.db` (macOS: `~/Library/Application Support/fynance/`)
- **Frontend**: React 19 + React Compiler + Vite + TypeScript + Tailwind + shadcn-ui + Recharts, embedded in the Rust binary via `include_dir!`
<!-- DEFERRED: - **AI**: Claude API via `reqwest` + `serde_json` (Haiku for categorization, Sonnet for analysis) -->
- **AI**: External agents handle categorization and data extraction, pushing pre-processed data through the REST API. The API is documented as an agent-readable spec (OpenAPI at `/api/docs`).
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
<!-- DEFERRED: | `reqwest` | HTTP client for Claude API | -->
| `reqwest` | HTTP client (future: external API integrations) |
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
| `dotenvy` | Load `.env` file for configuration |
| `ts-rs` | Auto-generate TypeScript types from Rust structs |

## Configuration

All runtime config via environment variables (loaded from `.env` via `dotenvy`). See `.env.example` for the full list. Key variables: `FYNANCE_PORT`, `FYNANCE_DB_PATH`, `FYNANCE_HOST`, `FYNANCE_LOG_LEVEL`.
<!-- DEFERRED: ANTHROPIC_API_KEY is not needed for MVP. Internal AI workflows are a future enhancement. -->

## Commands

- Build backend: `cargo build --release`
- Build frontend: `cd frontend && npm run build`
- Build everything: `make build` (frontend first, then cargo)
- Run: `cargo run --release -- <subcommand>`
- Dev backend: `cargo watch -x 'run -- serve --no-open'` (live reload)
- Dev frontend: `cd frontend && npm run dev` (HMR, proxies `/api/*` to backend)
- Test: `cargo test`
- Lint: `cargo clippy --all-targets -- -D warnings`
- Format: `cargo fmt`
- Binary: `./target/release/fynance`
- Docker (local build): `docker compose up -d --build`
- Docker (from GHCR): `docker compose pull && docker compose up -d`
- Docker update: `docker compose pull && docker compose up -d`

## CLI Subcommands

```bash
fynance serve [--port 7433] [--no-open]      # Start local web UI
fynance import <file|dir> --account <id>     # Import CSV statements (auto-detects bank format)
# DEFERRED: fynance categorize [--batch]      # Run categorization pipeline (internal AI, deferred to post-MVP)
fynance account add --id <id> --name <name> --institution <inst> --type <type>
fynance account set-balance <id> <amount> --date YYYY-MM-DD
fynance account list
fynance budget set --month YYYY-MM --category <c> --amount N
fynance budget status
fynance stats
fynance export --year YYYY --format csv
fynance monthly                               # import + categorize + snapshot
fynance token create --name <name>           # generate API token for programmatic access
fynance token list                            # list active tokens
fynance token revoke --name <name>           # revoke a token
```

## REST API Surface (served by `fynance serve`)

Browser UI requests from localhost need no auth. Programmatic access (scripts, agents) uses bearer token auth: `Authorization: Bearer fyn_...`

```
GET    /api/transactions?month=&category=&account=&page=&limit=
GET    /api/transactions/categories
GET    /api/transactions/accounts
PATCH  /api/transactions/:id                 # edit category, notes
POST   /api/import                           # typed JSON API for structured transaction data (agents, scripts)
POST   /api/import/csv                       # upload CSV (single file)
POST   /api/import/bulk                      # upload multiple CSVs
# DEFERRED: POST   /api/import/screenshot      # image -> Claude Vision -> transactions (internal AI, deferred)
# DEFERRED: POST   /api/categorize              # internal categorization pipeline (deferred, agents categorize externally)

GET    /api/budget/:month
POST   /api/budget
GET    /api/income/:month                    # derived from Income-category transactions

GET    /api/portfolio
GET    /api/portfolio/history
POST   /api/accounts
PATCH  /api/accounts/:id/balance

GET    /api/reports/:month
GET    /api/export?year=&format=             # csv or md (Obsidian-compatible)
GET    /api/docs                             # OpenAPI spec
```

## Security Model

This is a single-user local app. "Multi-user" means multiple OS users on the same machine, each running their own isolated instance.

- Axum binds to `127.0.0.1` by default. In Docker, `FYNANCE_HOST=0.0.0.0` is set so the port mapping works (Docker's network isolation is the boundary instead).
- Database path resolves from `dirs::data_local_dir()`, per OS user.
- Data directory created with mode `0o700`; DB file `0o600` on Unix.
<!-- DEFERRED: Internal AI security (not needed for MVP, no internal Claude API calls)
- Claude API key read from `ANTHROPIC_API_KEY` env var or `~/.config/fynance/config.yaml` (mode `600`). Never logged, never stored in DB.
- Claude API receives only normalized merchant strings, never amounts, dates, or account IDs.
-->
- No telemetry. No outbound calls from the binary. All AI processing happens in external agents that push data through the API.
- No auth for browser UI (loopback binding is the isolation boundary). Programmatic API access uses locally-generated bearer tokens (`fyn_` prefix, SHA-256 hashed in DB).

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
<!-- DEFERRED: - Claude API calls always go through `src/categorizer/claude.rs` so prompt caching and batch behavior stay consistent -->
- Axum handlers return `Result<Json<T>, AppError>` where `AppError` implements `IntoResponse`
- Frontend fetches through `src/api/client.ts`, never direct `fetch()` in components
- All API response types are auto-generated from Rust via `ts-rs` into `frontend/src/bindings/`. Never manually duplicate types in TypeScript.
