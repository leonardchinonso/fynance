# fynance

Personal finance tracker with a Rust backend and React web UI. Import bank CSV exports (Monzo, Revolut, Lloyds), track budgets and net worth, all from your browser. External AI agents handle categorization and data extraction via the REST API.

## Tech Stack

- **Backend:** Rust, Axum, SQLite (rusqlite), Tokio
- **Frontend:** React 19, React Compiler, Vite, TypeScript, Tailwind, shadcn-ui, Recharts
- **AI:** External agents categorize and extract data, pushing results through the API. Agent-readable OpenAPI docs at `/api/docs`.
- **Deployment:** Docker, single container, SQLite on a volume

## Deployment (Docker)

The simplest way to run fynance. Requires only Docker and Docker Compose.

1. Copy and configure the environment file:
   ```bash
   cp .env.example .env
   ```
   Edit `.env` and fill in your values (see [Environment Variables](#environment-variables) below).

2. Start the app:
   ```bash
   docker compose up -d
   ```

3. Open `http://localhost:7433` in your browser.

The database is created automatically on first run. Data persists in a Docker volume across restarts.

```bash
docker compose logs -f fynance   # view logs
docker compose down              # stop
docker compose down -v           # stop and delete all data
docker compose up -d --build     # rebuild locally after code changes
```

### Updating

Pre-built images are published to GitHub Container Registry on every push to `main`. To update a running deployment:

```bash
docker compose pull
docker compose up -d
```

To pin to a specific release version, set the image in `docker-compose.yml`:
```yaml
image: ghcr.io/<owner>/fynance:v0.3.0
```

### Releases

Releases are created manually via the GitHub Actions UI. Go to Actions > Release > Run workflow, enter a version tag (e.g., `v0.3.0`), and it builds, publishes the Docker image, and creates a GitHub Release with auto-generated release notes.

## Environment Variables

Configuration is managed through a `.env` file at the project root. The repo includes a `.env.example` with safe defaults. Copy it to `.env` and fill in your values. The `.env` file is gitignored and should never be committed.

| Variable | Default | Required | Description |
|---|---|---|---|
| `FYNANCE_PORT` | `7433` | No | HTTP server port (serves both web UI and REST API) |
| `FYNANCE_HOST` | `127.0.0.1` | No | Bind address. Set to `0.0.0.0` in Docker (done automatically by the Docker image) |
| `FYNANCE_DB_PATH` | OS data dir | No | Full path to the SQLite database file. In Docker this is set to `/home/fynance/data/fynance.db` automatically |
| `ANTHROPIC_API_KEY` | (none) | No | Not needed for MVP. Internal AI workflows are deferred. Categorization is handled by external AI agents that push data through the API |
| `FYNANCE_LOG_LEVEL` | `info` | No | Log verbosity. Options: `trace`, `debug`, `info`, `warn`, `error` |
| `FYNANCE_ADDITIONAL_DOCS` | (none) | No | Path to additional documentation for AI agents building against this environment |

Example `.env` for local development:
```env
FYNANCE_PORT=7433
FYNANCE_HOST=127.0.0.1
FYNANCE_LOG_LEVEL=debug
```

Example `.env` for Docker deployment:
```env
FYNANCE_PORT=7433
FYNANCE_LOG_LEVEL=info
```
(Docker sets `FYNANCE_HOST` and `FYNANCE_DB_PATH` automatically via the Dockerfile, no need to override them.)

## Local Development Setup

### Prerequisites

- Rust 1.85+ (`rustup install stable`)
- Node.js 22+ and npm
- `cargo-watch` for live reload: `cargo install cargo-watch`

### Getting started

1. Clone the repo and set up the environment:
   ```bash
   git clone <repo-url> && cd fynance-be
   cp .env.example .env
   # Edit .env with your values (see Environment Variables above)
   ```

2. Install frontend dependencies:
   ```bash
   cd frontend
   npm install
   cd ..
   ```

3. Do an initial frontend build (needed so `include_dir!` can compile):
   ```bash
   cd frontend && npm run build && cd ..
   ```

4. Start the Rust backend (terminal 1):
   ```bash
   cargo watch -x 'run -- serve --no-open'
   ```
   The API server starts on `http://localhost:7433` and auto-restarts when `.rs` files change.

5. Start the React frontend (terminal 2):
   ```bash
   cd frontend
   npm run dev
   ```
   Vite dev server starts on `http://localhost:5173` with hot module replacement. API calls (`/api/*`) are proxied to the Rust backend automatically.

6. Open `http://localhost:5173` in your browser.

### How local dev works

You work against two servers: Vite for the frontend (instant hot reload) and Axum for the API (recompiles on save via `cargo-watch`). The browser talks to Vite, which proxies API requests to Axum.

```
Browser (localhost:5173)
  |-- page/assets --> Vite (instant HMR)
  |-- /api/*      --> proxied to Axum (localhost:7433)
```

Frontend changes appear instantly. Backend changes take a few seconds to recompile.

## Build

```bash
make build           # frontend + backend (production)
cargo build          # backend only
cargo test           # run tests
cargo clippy --all-targets -- -D warnings   # lint
cargo fmt            # format
```

## CLI

```bash
fynance serve [--port 7433] [--no-open]      # Start local web UI
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

## How It Works

The Rust binary serves both the API and the frontend. At build time, Vite compiles the React app to static files, which get embedded into the Rust binary via `include_dir!`. At runtime, Axum serves everything from a single process: API routes at `/api/*`, the React app at everything else.

In development, the frontend runs on its own Vite dev server with hot reload, and proxies API calls to the Rust backend. No embedding happens during dev.
