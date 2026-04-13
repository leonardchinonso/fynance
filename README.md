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

- **Rust** 1.85+ MSRV: `curl https://sh.rustup.rs -sSf | sh`
- **Node.js** 22+ and **npm**: From [nodejs.org](https://nodejs.org)
- **`cargo-watch`** (optional, for live reload): `cargo install cargo-watch`
- **.env file** with API keys (see [Environment Variables](#environment-variables) above)

### Initial Setup

Only do this once:

```bash
# 1. Clone and enter the repo
git clone https://github.com/leonardchinonso/fynance.git
cd fynance

# 2. Copy and configure the environment file
cp .env.example .env
$EDITOR .env
# Fill in at least: FYNANCE_ANTHROPIC_API_KEY for CSV imports

# 3. Install frontend dependencies
cd frontend
npm install
cd ..

# 4. Do an initial frontend build (required for Rust embedded UI)
cd frontend && npm run build && cd ..
```

### Running the Full Stack (End-to-End Testing)

This is the recommended workflow for active development on both frontend and backend. You will have two dev servers running: Vite for the frontend and Axum for the backend. The frontend will proxy API calls to the backend, so you can interact with the real API while developing.

**Terminal 1** — Backend with live reload:
```bash
cd backend
cargo watch -x 'run -- serve --no-open'
```
The backend API starts on `http://localhost:7433` and auto-recompiles when `.rs` files change.

**Terminal 2** — Frontend with hot module replacement:
```bash
cd frontend
npm run dev
```
Vite dev server starts on `http://localhost:5173` with instant hot reload.

**Browser:**
```
Open http://localhost:5173
```

The frontend automatically proxies `/api/*` requests to the backend:
```
Browser (localhost:5173)
  ├── page/assets --> Vite dev server (instant HMR)
  └── /api/*      --> proxied to Axum backend (localhost:7433)
```

Frontend changes appear instantly. Backend changes take a few seconds to recompile.

### Running the Backend Only

Use this if you're only working on the Rust backend or want to test the embedded UI.

```bash
cd backend
cargo watch -x 'run -- serve --no-open'
```

Then open `http://localhost:7433` in your browser. The compiled React app is embedded in the binary.

Or, without live reload:
```bash
cd backend
cargo run --release -- serve
```

### Running the Frontend Only

Use this if you're only working on the React frontend and want to iterate on the UI without backend changes.

```bash
cd frontend
npm run dev
```

Open `http://localhost:5173`. The frontend is configured to proxy `/api/*` requests to `http://localhost:7433`, so you need a running backend (see above).

If the backend is not running, API calls will fail. You can point to a different backend by editing `frontend/vite.config.ts` (the `proxy` configuration).

### One-Command Full Build

To build both frontend and backend for production (or to verify everything compiles):

```bash
make build
```

This runs `npm run build` in the frontend folder, then `cargo build --release` in the backend. The result is a single binary in `backend/target/release/fynance` with the compiled React app embedded.

To build only the backend:
```bash
cd backend && cargo build --release
```

To build only the frontend:
```bash
cd frontend && npm run build
```

### Typical Development Workflows

#### Scenario 1: Backend Changes Only (Rust)

You have the backend running and want to iterate on the API or database logic.

```bash
# Terminal 1: Backend with live reload
cd backend && cargo watch -x 'run -- serve --no-open'

# Terminal 2: Optional, to make requests manually
curl http://localhost:7433/api/health
curl http://localhost:7433/api/docs    # OpenAPI schema
```

#### Scenario 2: Frontend Changes Only (React/TypeScript)

You have both frontend and backend running and want to iterate on the UI.

```bash
# Terminal 1: Backend (can be idle, requests will still work)
cd backend && cargo watch -x 'run -- serve --no-open'

# Terminal 2: Frontend with HMR
cd frontend && npm run dev

# Browser: Open http://localhost:5173
# Make changes to src/**/*.tsx - they appear instantly
```

#### Scenario 3: End-to-End Testing (Both Stacks)

You want to test the full flow: user interaction → API call → database → response → UI update.

Follow the instructions in [Running the Full Stack](#running-the-full-stack-end-to-end-testing) above.

#### Scenario 4: Testing the Production Binary

After a full build, you can run the compiled binary locally:

```bash
make build
./backend/target/release/fynance serve
```

Open `http://localhost:7433`. The frontend is embedded, so no separate dev server is needed. This is the same binary you would ship in Docker.

### Testing and Validation

Before pushing code, run tests and linters:

```bash
# All backend tests (no API key needed)
cd backend && cargo test

# Lint
cargo clippy --all-targets -- -D warnings

# Format check
cargo fmt --check
```

For the live smoke test against the real Anthropic API:
```bash
cd backend
FYNANCE_ANTHROPIC_API_KEY=<your-key> cargo test -- --ignored
```

### Importing Real Bank Data

Once the backend is running, you can import real CSV bank statements:

```bash
fynance account add --id my-monzo --name "Monzo" --institution Monzo --type checking --currency GBP
fynance import ~/Downloads/monzo-statement.csv --account my-monzo
fynance stats    # verify the import
```

The frontend will then show the imported transactions in the Transactions view and include them in budget/portfolio calculations.

### How It All Works Together

```
Development Flow:
├── Frontend (Vite @ localhost:5173)
│   ├── src/ (TypeScript + React)
│   ├── npm run build --> dist/
│   └── npm run dev   --> dev server with HMR & proxy
│
├── Backend (Axum @ localhost:7433)
│   ├── src/main.rs, lib.rs, ...
│   ├── cargo build --release --> fynance binary
│   └── cargo run -- serve    --> HTTP server
│
├── Database (SQLite)
│   └── ~/.local/share/fynance/fynance.db (macOS: ~/Library/Application Support/fynance/)
│
└── Browser
    ├── Request GET /app/transactions
    ├── Vite responds with React bundle
    ├── React mounts, fetches GET /api/transactions
    ├── Proxy sends to Axum backend
    ├── Axum queries SQLite, returns JSON
    └── React renders the data
```

In production:
- Build frontend with `npm run build`.
- Embed the `dist/` folder into the Rust binary via `include_dir!`.
- Compile the Rust binary: `cargo build --release`.
- Run the single binary: `./target/release/fynance serve`.
- Everything is served from `http://localhost:7433` with no separate dev servers.

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

## Project Structure

```
fynance/
├── frontend/                # React 19 app (see frontend/README.md for full structure)
│   └── src/
│       └── types/           # TypeScript interfaces (auto-generated by ts-rs from Rust later)
├── backend/                 # Rust crate (see backend/README.md)
│   ├── src/
│   └── config/              # categories.yaml, rules.yaml
├── db/                      # SQLite schema and migrations (see db/README.md)
├── assets/                  # Shared assets (logo, etc.)
├── docs/                    # Design docs, plans, research
│   ├── design/
│   ├── plans/
│   └── research/
├── .github/workflows/       # CI/CD
├── docker-compose.yml
├── Dockerfile
├── Makefile
├── .env.example
├── CLAUDE.md                # AI agent context for this project
└── README.md
```

## How It Works

The Rust binary serves both the API and the frontend. At build time, Vite compiles the React app to static files, which get embedded into the Rust binary via `include_dir!`. At runtime, Axum serves everything from a single process: API routes at `/api/*`, the React app at everything else.

In development, the frontend runs on its own Vite dev server with hot reload, and proxies API calls to the Rust backend. No embedding happens during dev.
