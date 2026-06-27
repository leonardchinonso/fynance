<p align="right"><a href="https://github.com/leonardchinonso/fynance/releases/latest"><img src="https://img.shields.io/github/v/release/leonardchinonso/fynance?label=latest%20version&logo=github&logoColor=white" alt="Latest version"></a> <a href="https://github.com/leonardchinonso/fynance/actions/workflows/release.yml"><img src="https://img.shields.io/github/commits-since/leonardchinonso/fynance/latest?label=unreleased%20commits&logo=git&logoColor=white" alt="Unreleased commits"></a> <a href="https://github.com/leonardchinonso/fynance/actions/workflows/release.yml"><img src="https://img.shields.io/github/actions/workflow/status/leonardchinonso/fynance/release.yml?branch=master&label=latest%20build%20status&logo=githubactions&logoColor=white" alt="Latest build status"></a></p>

# fynance

Personal finance tracker with a Rust backend and React web UI. Import bank CSV exports (Monzo, Revolut, Lloyds), track budgets and net worth, all from your browser. External AI agents handle categorization and data extraction via the REST API.

<p align="center">
  <a href="https://fynance-3c.vercel.app">
    <img src="https://img.shields.io/badge/Live%20Demo-Try%20it%20now-1a9e8f?style=for-the-badge&logoColor=white" alt="Live Demo" />
  </a>
</p>

## Running fynance

### Option 1: Local binary (personal use on your own machine)

**Prerequisites:** Nothing. Just download the binary and run it. No Rust, no Node.js, no Docker.

Download the binary for your platform from [GitHub Releases](https://github.com/leonardchinonso/fynance/releases):

| Platform | File |
|---|---|
| Linux (x86_64) | `fynance-linux-x86_64` |
| macOS (Apple Silicon) | `fynance-macos-aarch64` |
| Windows | `fynance-windows-x86_64.exe` |

**Linux / macOS:**
```bash
chmod +x fynance-*        # make it executable
sudo mv fynance-* /usr/local/bin/fynance   # put it on your PATH
fynance serve
```

**Windows:**

Rename the downloaded file to `fynance.exe`, move it somewhere on your PATH (e.g. `C:\Users\<you>\bin\`), then run:
```powershell
fynance serve
```

The app opens automatically in your browser at `http://localhost:7433`. Your data is stored in the OS default location:
- Linux: `~/.local/share/fynance/fynance.db`
- macOS: `~/Library/Application Support/fynance/`
- Windows: `%APPDATA%\fynance\`

#### Updating

Download the latest binary from [GitHub Releases](https://github.com/leonardchinonso/fynance/releases) and replace the existing one on your PATH.

### Option 2: Docker (always-on, self-hosted)

**Prerequisites:** [Docker](https://docs.docker.com/get-docker/) and Docker Compose. No Rust, no Node.js, no build step.

Use this if you want fynance running persistently on a home server or NAS, surviving reboots automatically.

Create a `docker-compose.yml`:

```yaml
services:
  fynance:
    image: ghcr.io/leonardchinonso/fynance:latest
    ports:
      - "7433:7433"
    volumes:
      - fynance-data:/home/fynance/data
    environment:
      - FYNANCE_HOST=0.0.0.0
      - FYNANCE_DB_PATH=/home/fynance/data/fynance.db
      - FYNANCE_LOG_LEVEL=info
    restart: unless-stopped

volumes:
  fynance-data:
```

Then run:

```bash
docker compose up -d
```

Open `http://localhost:7433` in your browser. The database is created automatically and persists in a Docker volume across restarts.

**Auth token (required for Docker deployments):**

When running via Docker, all API requests require a bearer token. Generate one after starting the container:

```bash
docker exec <container_name> fynance token create --name browser
```

Copy the output (`fyn_...`) and paste it into **Settings > Auth** in the web UI. The token is stored in your browser and sent automatically with every request. Run `docker compose ps` to find the container name.

```bash
docker compose logs -f fynance   # view logs
docker compose down              # stop
docker compose down -v           # stop and delete all data
```

> **Note:** `FYNANCE_HOST=0.0.0.0` is required in Docker so that port mapping can reach the server. If you are deploying behind a reverse proxy and want to restrict which network interface accepts connections, bind the port to a specific IP instead, for example `"192.168.1.49:7433:7433"`.

#### Updating

Pre-built images are published to GitHub Container Registry on each tagged release:

```bash
docker compose pull
docker compose up -d
```

To pin to a specific release:
```yaml
image: ghcr.io/leonardchinonso/fynance:v0.9.0
```

### Environment Variables

Configuration is managed through a `.env` file at the project root. The repo includes a `.env.example` with safe defaults. Copy it to `.env` and fill in your values. The `.env` file is gitignored and should never be committed.

| Variable | Default | Required | Description |
|---|---|---|---|
| `FYNANCE_PORT` | `7433` | No | HTTP server port (serves both web UI and REST API) |
| `FYNANCE_HOST` | `127.0.0.1` | No | Bind address. Set to `0.0.0.0` in Docker so port mapping can reach the server |
| `FYNANCE_DB_PATH` | OS data dir | No | Full path to the SQLite database file. In Docker set to `/home/fynance/data/fynance.db` |
| `FYNANCE_LOG_LEVEL` | `info` | No | Log verbosity. Options: `trace`, `debug`, `info`, `warn`, `error` |
| `FYNANCE_ANTHROPIC_API_KEY` | (none) | One of these two | Console API key (`sk-ant-api...`) from [console.anthropic.com](https://console.anthropic.com/). Pay-per-token billing |
| `FYNANCE_CLAUDE_CODE_OAUTH_TOKEN` | (none) | One of these two | Claude Pro/Max subscription token (`sk-ant-oat01...`) from `claude setup-token`. Billed against your subscription's Agent SDK credit pool, not per-token |
| `FYNANCE_IMPORT_LLM_MODEL` | `claude-haiku-4-5-20251001` | No | Claude model used by the CSV/statement parser |
| `FYNANCE_IMPORT_MIN_DETECT_CONF` | `0.80` | No | File-level detection confidence threshold (0.0 to 1.0). Import fails hard below this |
| `FYNANCE_IMPORT_MIN_ROW_CONF` | `0.70` | No | Row-level confidence threshold (0.0 to 1.0). Rows below this are skipped with a warning |
| `FYNANCE_PARSE_PDF_MODEL` | `claude-sonnet-4-6` | No | More capable model used for PDF/visual document parsing |
| `FYNANCE_PARSE_PROVIDER` | `anthropic` | No | LLM provider for `/api/parse` and import: `anthropic` (default) or `openai` |
| `FYNANCE_OPENAI_API_KEY` / `_TEXT_MODEL` / `_PDF_MODEL` | (none) | No | OpenAI credentials and models, used only when `FYNANCE_PARSE_PROVIDER=openai`. See `.env.example` |
| `VITE_MOCK_ONLY` | (none) | No | Frontend build flag: force mock-data mode for demo/preview deployments |

**LLM credentials.** Importing statements requires an Anthropic credential. You can use either a pay-per-token Console API key (`FYNANCE_ANTHROPIC_API_KEY`) or a Claude Pro/Max subscription token (`FYNANCE_CLAUDE_CODE_OAUTH_TOKEN`, obtained by running `claude setup-token`). The subscription token draws from your plan's monthly Agent SDK credit instead of billing per token, which is much cheaper for large imports. When both are set, the subscription token is used first and the API key is an automatic fallback if it is rejected or its credit is exhausted. The model, prompts, output, streaming, and PDF support are identical either way.

> Note: using a subscription OAuth token from a third-party app (rather than Claude Code itself) falls outside Anthropic's Consumer Terms for that token type. Only set `FYNANCE_CLAUDE_CODE_OAUTH_TOKEN` if you accept that.

Example `.env` for local binary use:
```env
FYNANCE_PORT=7433
FYNANCE_HOST=127.0.0.1
FYNANCE_LOG_LEVEL=info
```

Example `.env` for Docker deployment:
```env
FYNANCE_PORT=7433
FYNANCE_LOG_LEVEL=info
```
(Docker sets `FYNANCE_HOST` and `FYNANCE_DB_PATH` via the compose file, no need to override them.)

## Releases

Releases are cut manually from the **Release** workflow (`workflow_dispatch`), not automatically on merge. The workflow tags the commit (an explicit version, or an auto-incremented minor from the latest tag), builds the Linux binary (macOS and Windows are opt-in to save Actions minutes), pushes the Docker image to GHCR with `latest` and the version tag, and publishes a GitHub Release with auto-generated notes and the compiled binaries attached.

## CLI

```bash
fynance serve [--port 7433] [--no-open]      # Start local web UI
fynance import <file|dir> --account <id>     # Import CSV statements
fynance account add --id <id> --name <name> --institution <inst> --type <type> --profile <id> [--currency GBP]
fynance account set-balance <id> <amount> --date YYYY-MM-DD
fynance account list
fynance account delete <id> [--hard]         # soft-delete (deactivate); --hard removes the row
fynance transaction delete <id>... [--account <id>]   # hard-delete txns by id, or all for an account
fynance budget set --month YYYY-MM --category <c> --amount N
fynance budget status --month YYYY-MM
fynance stats
fynance token create --name <name>           # generate API token for programmatic access
fynance token list
fynance token revoke --name <name>
```

## Local Development Setup

> **Important:** During development always open `http://localhost:5173` (Vite dev server), not `http://localhost:7433`. The backend port serves a pre-compiled frontend bundle only updated when you run `make build`. The Vite dev server reflects your latest source changes instantly via HMR.

### Tech Stack

- **Backend:** Rust (edition 2024, MSRV 1.85), Axum, SQLite via rusqlite, Tokio
- **Frontend:** React 19, React Compiler, Vite, TypeScript, Tailwind, shadcn-ui, Recharts
- **AI:** External agents categorize and extract data, pushing results through the REST API. Agent-readable OpenAPI docs at `/api/docs`.
- **Deployment:** Standalone binary or Docker, SQLite on disk or a volume

### Prerequisites

- **Rust** 1.85+: `curl https://sh.rustup.rs -sSf | sh`
- **Node.js** 22+ and **npm**: From [nodejs.org](https://nodejs.org)
- **`cargo-watch`** (optional, for live reload): `cargo install cargo-watch`

### Initial Setup

Only do this once:

```bash
# 1. Clone and enter the repo
git clone https://github.com/leonardchinonso/fynance.git
cd fynance

# 2. Copy and configure the environment file
cp .env.example .env
$EDITOR .env

# 3. Install frontend dependencies
cd frontend && npm install && cd ..

# 4. Do an initial frontend build (required for the Rust embedded UI)
cd frontend && npm run build && cd ..
```

### Running the Full Stack (End-to-End Testing)

The recommended workflow for active development on both frontend and backend. Two dev servers run simultaneously: Vite for the frontend and Axum for the backend. The frontend proxies API calls to the backend so you interact with the real API while developing.

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

Open `http://localhost:5173`. The frontend automatically proxies `/api/*` requests to the backend:
```
Browser (localhost:5173)
  ├── page/assets --> Vite dev server (instant HMR)
  └── /api/*      --> proxied to Axum backend (localhost:7433)
```

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

Open `http://localhost:5173`. The frontend proxies `/api/*` requests to `http://localhost:7433`, so a running backend is required. If it is not running, API calls will fail. You can point to a different backend by editing the `proxy` configuration in `frontend/vite.config.ts`.

### Build

```bash
make build           # frontend + backend (production)
cargo build          # backend only
cd frontend && npm run build   # frontend only
```

`make build` runs `npm run build` in the frontend folder then `cargo build --release` in the backend. The result is a single binary at `backend/target/release/fynance` with the compiled React app embedded.

### Testing and Validation

PRs that fail CI will not be merged. Before pushing, run:

```bash
cd backend && cargo test                          # all backend tests
cargo clippy --all-targets -- -D warnings         # lint (zero warnings enforced)
cargo fmt --check                                 # formatting
```

For the live smoke test against the real Anthropic API:
```bash
cd backend
FYNANCE_ANTHROPIC_API_KEY=<your-key> cargo test -- --ignored
```

### How It All Works Together

```
Development:
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
│   └── ~/.local/share/fynance/fynance.db
│
└── Browser
    ├── React bundle served by Vite (dev) or Axum (prod)
    ├── React fetches GET /api/transactions
    ├── Axum queries SQLite, returns JSON
    └── React renders the data
```

### Project Structure

```
fynance/
├── frontend/                # React 19 app (see frontend/README.md)
│   └── src/
│       └── types/           # TypeScript interfaces (auto-generated by ts-rs from Rust)
├── backend/                 # Rust crate (see backend/README.md)
│   ├── src/
│   └── config/              # categories.yaml, rules.yaml
├── db/                      # SQLite schema and migrations
├── assets/                  # Shared assets (logo, etc.)
├── docs/                    # Design docs, plans, research
├── .github/workflows/       # CI/CD
├── docker-compose.yml
├── Dockerfile
├── Makefile
├── .env.example
└── README.md
```

### How It Works

The Rust binary serves both the API and the frontend from a single process and port. At build time, Vite compiles the React app to static files, which get embedded into the Rust binary via `include_dir!`. At runtime, Axum serves API routes at `/api/*` and the React app at everything else.

In development, the frontend runs on its own Vite dev server with hot reload, and proxies API calls to the Rust backend. No embedding happens during dev.
