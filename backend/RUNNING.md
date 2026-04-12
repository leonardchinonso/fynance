# Running the fynance Backend

This document covers everything you need to build, configure, and run the fynance Rust backend from scratch. It is intended as the single reference for development setup, day-to-day usage, and common workflows.

## Prerequisites

| Tool | Minimum version | Install |
|---|---|---|
| Rust toolchain | 1.85 (MSRV) | `curl https://sh.rustup.rs -sSf \| sh` |
| `cargo` | ships with Rust | — |
| `cargo-watch` (optional) | any | `cargo install cargo-watch` |

No other system dependencies are required. The SQLite library is bundled via the `rusqlite` `bundled` feature and compiled into the binary automatically. The React frontend is compiled and embedded in the binary via `include_dir!`.

## Project layout

```
fynance/
├── backend/          # Rust crate (this document lives here)
│   ├── Cargo.toml
│   ├── src/
│   │   ├── main.rs
│   │   ├── lib.rs
│   │   ├── cli.rs            # Clap argument definitions
│   │   ├── model.rs          # Domain types (Transaction, Account, BankFormat, …)
│   │   ├── util.rs           # Shared helpers (fingerprint, parse_date, parse_amount)
│   │   ├── commands/         # One file per CLI subcommand
│   │   │   ├── serve.rs      # HTTP server startup and browser launch
│   │   │   ├── token.rs      # Token create / list / revoke
│   │   │   └── (others)      # account, import, stats, budget
│   │   ├── importers/        # CSV import pipeline
│   │   │   ├── mod.rs        # Importer trait + get_importer
│   │   │   ├── csv_importer.rs
│   │   │   ├── llm_parser.rs # LLM-based statement parser
│   │   │   └── unified.rs    # UnifiedStatementRow schema
│   │   ├── server/           # Axum HTTP server
│   │   │   ├── mod.rs        # Server setup, middleware wiring
│   │   │   ├── auth.rs       # Loopback-trust bearer token auth middleware
│   │   │   ├── error.rs      # Error type implementing IntoResponse
│   │   │   ├── state.rs      # Shared server state (Db)
│   │   │   ├── static_files.rs # Embedded React bundle serving
│   │   │   └── routes/
│   │   │       ├── mod.rs    # Router setup
│   │   │       ├── health.rs # GET /health
│   │   │       ├── docs.rs   # GET /api/docs (OpenAPI 3.1 spec)
│   │   │       └── (future)  # API routes (transactions, budget, etc.)
│   │   └── storage/          # SQLite persistence (Db type)
│   ├── config/
│   │   ├── categories.yaml   # Spending category taxonomy
│   │   └── prompts/
│   │       └── statement_parser.txt  # LLM system prompt (embedded at compile time)
│   └── tests/                # Integration tests + fixtures
├── db/
│   └── sql/
│       ├── schema.sql        # Full schema, run on every Db::open (idempotent)
│       └── migrations/       # Additive ALTER TABLE migrations run once on startup
├── .env.example              # All supported env vars with documentation
└── Makefile                  # Convenience targets (see below)
```

## Quick start

```bash
# 1. Clone and enter the repo
git clone https://github.com/leonardchinonso/fynance.git
cd fynance

# 2. Copy the example env file and fill in your values
cp .env.example .env
$EDITOR .env

# 3. Build the release binary (frontend must be built first for the embedded UI)
make build           # builds frontend then cargo --release
# or, backend only:
cd backend && cargo build --release

# 4. Run
./backend/target/release/fynance --help
```

## Configuration

All runtime configuration is done via environment variables. The binary loads `.env` from the current working directory automatically on startup (via `dotenvy`). Every variable has a sensible default so you only need to set the ones you actually want to change.

| Variable | Default | Description |
|---|---|---|
| `FYNANCE_DB_PATH` | OS data dir (see below) | Full path to the SQLite database file |
| `FYNANCE_PORT` | `7433` | HTTP server port |
| `FYNANCE_HOST` | `127.0.0.1` | HTTP bind address. Set to `0.0.0.0` in Docker |
| `FYNANCE_LOG_LEVEL` | `info` | Log verbosity: `trace`, `debug`, `info`, `warn`, `error`. Also respected via `RUST_LOG` |
| `FYNANCE_ANTHROPIC_API_KEY` | — | Required for `fynance import`. Anthropic API key for LLM-based CSV parsing |
| `FYNANCE_IMPORT_LLM_MODEL` | `claude-haiku-4-5-20251001` | Claude model used by the CSV parser |
| `FYNANCE_IMPORT_MIN_DETECT_CONF` | `0.80` | File-level confidence threshold. Import fails hard below this |
| `FYNANCE_IMPORT_MIN_ROW_CONF` | `0.70` | Row-level confidence threshold. Rows below this are skipped with a warning |

### Default database path

The binary resolves the database path from the OS-native data directory:

| OS | Default path |
|---|---|
| macOS | `~/Library/Application Support/fynance/fynance.db` |
| Linux | `~/.local/share/fynance/fynance.db` |
| Windows | `%APPDATA%\fynance\fynance.db` |

Override with `--db <path>` (CLI flag) or `FYNANCE_DB_PATH` (env var). The CLI flag takes precedence.

The parent directory is created with mode `0700` and the database file with mode `0600` on Unix so that other OS users on the same machine cannot read another user's transactions.

## Database setup

There is no separate migration step. The database is created and initialized automatically the first time you run any `fynance` command:

1. `Db::open` runs `db/sql/schema.sql` (all `CREATE TABLE IF NOT EXISTS` statements, fully idempotent).
2. `Db::open` then runs each migration in `db/sql/migrations/` once, guarded by a `PRAGMA table_info` check so that re-running the binary never double-applies a migration.

You do not need to do anything special to set up the database.

## HTTP Server and API

The server is started with `fynance serve`. It runs on `127.0.0.1:7433` by default and serves:

- Embedded React UI at `/` and `/app/*`
- Health check at `GET /health`
- OpenAPI documentation at `GET /api/docs` (machine-readable schema for agents)
- API routes at `/api/*` (transaction, budget, portfolio endpoints — phase 2+)

### Authentication

Use `FYNANCE_HOST=0.0.0.0` (or set in `.env`) to bind to a non-loopback address (Docker, network access). In this case, all requests require bearer token authentication:

```bash
# Browser will be prompted to auth, or use curl:
curl -H "Authorization: Bearer fyn_abc123..." http://0.0.0.0:7433/api/health
```

See the `token create` / `token list` / `token revoke` commands above for token management.

## CLI subcommands

All subcommands accept a global `--db <path>` flag to override the default database location.

### `fynance serve`

Start the local Axum HTTP server and open the browser UI.

```bash
fynance serve
fynance serve --port 8080
fynance serve --no-open      # skip auto-opening the browser
```

The server binds to `127.0.0.1` by default (loopback only). The compiled React frontend is embedded in the binary via `include_dir!` and served as static files; no separate frontend process is needed in production.

#### Authentication

The server uses a **loopback-trust auth model**:

- **Loopback access** (from `127.0.0.1`): No authentication required. The browser UI is served and all API routes are accessible without a token.
- **Non-loopback access** (Docker, remote, `0.0.0.0`): Requires bearer token auth. All API requests must include `Authorization: Bearer fyn_<token>` header.

Browser requests are automatically trusted when the server is on loopback. Programmatic access (scripts, external agents) uses tokens generated with `fynance token create`. The full API is documented in machine-readable form at `GET /api/docs` (OpenAPI 3.1 schema with embedded category taxonomy).

### `fynance token`

Manage API bearer tokens for programmatic access.

```bash
# Generate a new token
fynance token create --name my-agent

# List all active tokens
fynance token list

# Revoke a token by name
fynance token revoke --name my-agent
```

Tokens are generated as random strings (prefix `fyn_`) and stored as SHA-256 hashes in the database. The plaintext token is only shown once at creation time. Use `token list` to see which tokens are active; use `token revoke` to invalidate a token without deleting it (it remains in the DB but is marked revoked).

### `fynance account`

Register and inspect accounts. An account must exist before transactions can be imported into it.

```bash
# Register a new account (or update an existing one)
fynance account add \
  --id monzo-current \
  --name "Monzo Current Account" \
  --institution Monzo \
  --type checking \
  --currency GBP

# Supported account types: checking, savings, investment, credit, cash, pension

# Record a balance snapshot (also updates the portfolio_snapshots table)
fynance account set-balance monzo-current 2491.70 --date 2026-03-31

# List all registered accounts
fynance account list
```

### `fynance import`

Import a CSV bank statement (or a directory of CSVs) into the database. Requires `FYNANCE_ANTHROPIC_API_KEY` to be set.

```bash
# Single file
fynance import ~/Downloads/monzo-march.csv --account monzo-current

# Entire directory (all *.csv files, sorted alphabetically)
fynance import ~/Downloads/statements/ --account monzo-current
```

The import pipeline:
1. Reads the file to a string.
2. Sends it to the Anthropic API using the system prompt in `backend/config/prompts/statement_parser.txt`.
3. Receives a structured `ParsedStatement` (bank name, confidence, and one `UnifiedStatementRow` per transaction).
4. Applies two confidence gates:
   - **File-level**: if `detection_confidence` < `FYNANCE_IMPORT_MIN_DETECT_CONF` (default 0.80), the import fails hard with an error.
   - **Row-level**: rows with `row_confidence` < `FYNANCE_IMPORT_MIN_ROW_CONF` (default 0.70) are skipped with a warning; the rest of the file continues.
5. Fingerprints each accepted row (`sha256(date|amount|description|account_id)`) and inserts it with `INSERT OR IGNORE`, so re-importing the same file is always idempotent.

Output example:
```
monzo-march.csv: 42 new, 0 duplicates [monzo (97%)]
Totals: 42 new, 0 duplicates across 42 rows
```

### `fynance stats`

Print a quick summary of what is in the database.

```bash
fynance stats
```

Output example:
```
Total: 126 transactions (2026-01-01 to 2026-03-31)
  monzo-current: 84 (2026-01-01..2026-03-31) | uncategorized: 12
  revolut-main:  42 (2026-02-01..2026-03-31) | uncategorized: 3
```

### `fynance budget`

Set and inspect monthly budget targets.

```bash
# Set a category budget for a month
fynance budget set --month 2026-03 --category Groceries --amount 400

# View all budgets set for a month
fynance budget status --month 2026-03
```

Budget categories should match those in `backend/config/categories.yaml`. The full taxonomy is a two-level hierarchy: `Parent: Child` strings such as `Food: Groceries` or `Transport: Public Transit`.

## Makefile targets

Run from the project root (`fynance/`, not `fynance/backend/`):

```bash
make build          # Build frontend then backend (release)
make dev-backend    # Live-reload backend with cargo-watch (requires cargo-watch)
make dev-frontend   # Vite HMR dev server for the frontend
make test           # Run all backend tests
make lint           # cargo clippy --all-targets -- -D warnings
make fmt            # cargo fmt
make clean          # Remove build artifacts and frontend dist
```

## Running tests

```bash
cd backend

# All tests (unit + integration), no API key needed
cargo test

# With output from passing tests
cargo test -- --nocapture

# Live smoke test against the real Anthropic API (requires FYNANCE_ANTHROPIC_API_KEY)
FYNANCE_ANTHROPIC_API_KEY=<your-key> cargo test -- --ignored
```

The integration tests in `tests/import_csv.rs` use `MockStatementParser` seeded from JSON fixtures in `tests/fixtures/*.expected.json`, so they run without a network connection or API key. The `#[ignore]` live smoke test in `tests/llm_parser_live.rs` is the only test that hits the real API.

The `tests/server_smoke.rs` contains smoke tests for the HTTP server, including auth middleware and static file serving. These run without special configuration.

## Development workflow

### Backend only (no frontend changes)

```bash
cd backend

# Iterate quickly with live reload (for serve or any subcommand)
cargo watch -x 'run -- serve --no-open'

# Or test a single subcommand directly (no live reload)
cargo run -- stats
cargo run -- import ~/Downloads/monzo.csv --account monzo-current

# Access the server at http://localhost:7433 or via API:
curl http://localhost:7433/api/docs  # OpenAPI schema
curl http://localhost:7433/health     # Health check
```

### Full stack (backend + frontend)

```bash
# Terminal 1: backend with live reload
make dev-backend

# Terminal 2: frontend with HMR (proxies /api/* to the backend)
make dev-frontend
```

The Vite dev server proxies all `/api/*` requests to the backend so the frontend can be developed with hot module replacement while talking to a real (or seeded) backend.

### Adjusting the LLM import prompt

The system prompt is pinned in the repo at `backend/config/prompts/statement_parser.txt` and embedded at compile time via `include_str!`. To change it:

1. Edit `backend/config/prompts/statement_parser.txt`.
2. Recompile (`cargo build`).
3. Validate the change manually with the live smoke test:
   ```bash
   FYNANCE_ANTHROPIC_API_KEY=<key> cargo test -- --ignored
   ```

### API Documentation for Agents

External agents access the API at `GET /api/docs`. The OpenAPI 3.1 schema includes:

- All endpoint definitions with request/response schemas
- The full spending category taxonomy (two-level hierarchy: `Parent: Child`)
- Bearer token authentication requirements
- Examples for each endpoint

Agents should read this schema on startup or at each session to stay in sync with the running server. The category taxonomy is embedded in the spec so agents always know the valid categories without a separate configuration request.

Sample fetch:
```bash
curl http://localhost:7433/api/docs | jq '.components.schemas.Category'
```

## Logging

Log output goes to stderr and is controlled by `FYNANCE_LOG_LEVEL` (or `RUST_LOG` for full `tracing` filter syntax).

```bash
# More verbose
FYNANCE_LOG_LEVEL=debug fynance import ~/Downloads/monzo.csv --account monzo-current

# Trace all SQL and HTTP
RUST_LOG=fynance=trace fynance serve
```

Per the security conventions in `CLAUDE.md`, raw transaction descriptions are never logged at INFO level. The LLM request/response payload is logged at DEBUG only, and the response preview is truncated to 300 bytes.

## Troubleshooting

### `FYNANCE_ANTHROPIC_API_KEY is not set`

The import command requires an Anthropic API key. Add it to your `.env` file:

```
FYNANCE_ANTHROPIC_API_KEY=sk-ant-...
```

Get a key from [console.anthropic.com](https://console.anthropic.com/).

### `LLM detection confidence X.XX is below threshold 0.80`

The file you are trying to import does not look like a bank statement to the LLM. Check that:
- You are passing the right file (not a shopping list or an invoice).
- The file is a plain CSV, not an OFX, PDF, or QFX.

If you are testing with a real bank CSV and the threshold is too strict, lower it temporarily:
```bash
FYNANCE_IMPORT_MIN_DETECT_CONF=0.60 fynance import file.csv --account my-account
```

### `could not resolve OS data directory`

This happens in minimal container environments without a proper home directory. Set `FYNANCE_DB_PATH` explicitly:

```bash
FYNANCE_DB_PATH=/data/fynance.db fynance serve
```

### Port already in use

Change the port with `--port` or `FYNANCE_PORT`:

```bash
fynance serve --port 8080
# or
FYNANCE_PORT=8080 fynance serve
```

### Database is locked

The binary uses WAL journal mode. Only one writer is allowed at a time, but multiple readers are fine. If you see lock errors, make sure you do not have two instances of `fynance serve` running against the same database file.

### Authentication errors: `missing or invalid bearer token`

This happens when you bind to a non-loopback address (Docker, remote network, `0.0.0.0`). The server requires bearer token auth in this case.

Generate a token:
```bash
fynance token create --name my-api
```

Use it in API requests:
```bash
curl -H "Authorization: Bearer fyn_abc123..." http://0.0.0.0:7433/api/health
```

For loopback-only access, no token is needed. The default `FYNANCE_HOST=127.0.0.1` requires no auth.

### Remote access in Docker

If you are running the server inside a container:

1. Set `FYNANCE_HOST=0.0.0.0` in the `.env` file (or as a build argument).
2. Generate an API token with `fynance token create`.
3. Pass the token in all API requests: `Authorization: Bearer fyn_...`.

The server will still open the browser on container startup, but that URL is only accessible from the container host (port mapping).
