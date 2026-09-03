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

This is the recommended option if you only want to run fynance on your own computer. The steps below assume no command-line experience.

#### Step 1: Download the binary

Download the file for your platform from [GitHub Releases](https://github.com/leonardchinonso/fynance/releases) (open the latest release and look under **Assets**):

| Platform | File |
|---|---|
| Linux (x86_64) | `fynance-linux-x86_64` |
| macOS (Apple Silicon) | `fynance-macos-aarch64` |
| Windows | `fynance-windows-x86_64.exe` |

The whole app, the web interface and the command-line tool, is this single file. There is nothing else to install.

#### Step 2: Unblock the file (first run only)

The binaries are not signed by Apple or Microsoft, so your OS will warn you the first time you run a downloaded program. This is expected. You only have to do this once per downloaded file.

**Windows:** Windows SmartScreen may show *"Windows protected your PC."* Click **More info**, then **Run anyway**. (Alternatively, right-click the file → **Properties** → tick **Unblock** at the bottom → **OK**.)

**macOS:** Gatekeeper will say the file *"cannot be opened because the developer cannot be verified."* Clear the quarantine flag, then it will run normally:
```bash
xattr -d com.apple.quarantine fynance-macos-aarch64
```
(Alternatively, find the file in Finder, right-click it → **Open** → confirm in the dialog. You only need to do this once.)

#### Step 3: Make it a command you can run from anywhere (recommended)

This lets you type `fynance` instead of the full path to the file every time.

**Windows (PowerShell):** rename the file to `fynance.exe` and move it into a folder, then add that folder to your PATH:
```powershell
# Create a folder for it and move the download there (adjust the source path)
New-Item -ItemType Directory -Force "$HOME\bin"
Move-Item "$HOME\Downloads\fynance-windows-x86_64.exe" "$HOME\bin\fynance.exe"

# Add that folder to your user PATH (one time; restart the terminal afterwards)
[Environment]::SetEnvironmentVariable(
  "Path",
  [Environment]::GetEnvironmentVariable("Path", "User") + ";$HOME\bin",
  "User")
```

**Linux / macOS:**
```bash
chmod +x fynance-*                          # make it executable
sudo mv fynance-* /usr/local/bin/fynance    # put it on your PATH (rename to "fynance")
```

> Prefer not to move it? You can always run the file in place by giving its full path instead of `fynance`. On Windows PowerShell that looks like `& "C:\Users\you\Downloads\fynance-windows-x86_64.exe" serve`; on macOS/Linux, `./fynance-macos-aarch64 serve` from the folder it's in.

#### Step 4: Start the app

Open a terminal (PowerShell on Windows, Terminal on macOS) and run:
```bash
fynance serve
```
This starts the local server and **opens the app automatically in your browser** at `http://localhost:7433`. Leave the terminal window open while you use fynance, closing it stops the server. Press `Ctrl+C` in that window to stop it.

Useful options:
```bash
fynance serve --port 8080    # use a different port (default 7433)
fynance serve --no-open      # start without auto-opening the browser
```

The app starts empty on first run. Your data lives in a single SQLite file at the OS default location:
- Linux: `~/.local/share/fynance/fynance.db`
- macOS: `~/Library/Application Support/fynance/fynance.db`
- Windows: `%LOCALAPPDATA%\fynance\fynance.db` (i.e. `C:\Users\<you>\AppData\Local\fynance\`)

To use a database file somewhere else (for example an existing one), point to it with `--db`:
```bash
fynance serve --db "C:\path\to\fynance.db"     # Windows
fynance serve --db "/path/to/fynance.db"       # macOS / Linux
```

#### Step 5: Configure settings (optional)

Most people never need to change anything, the defaults work. When you do (a different port, a custom database location, or an Anthropic key so the in-app statement importer works), see [Configuring the standalone binary](#configuring-the-standalone-binary) below for exactly where settings go. The full list of variables is in the [Environment Variables](#environment-variables) table.

#### Updating

Download the latest binary from [GitHub Releases](https://github.com/leonardchinonso/fynance/releases) and replace the existing one (overwrite the `fynance` / `fynance.exe` file you put on your PATH). Your database is stored separately, so updating the binary never touches your data. On macOS you will need to clear the quarantine flag again (Step 2) on the new download.

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

### Configuring the standalone binary

Settings are read from **environment variables**. You do not edit anything inside the binary itself. There are three ways to set them, in order of how easy they are for non-developers. Real environment variables take precedence over a `.env` file, so anything you set on the command line wins.

**1. Command-line flags (simplest, for the common settings).** The two settings people change most often have dedicated flags, so no configuration file is needed at all:
```bash
fynance serve --port 8080 --db "C:\path\to\fynance.db"
```

**2. A `.env` file.** Create a plain text file named exactly `.env` (no other extension) containing `NAME=value` lines, one per setting:
```env
FYNANCE_PORT=8080
FYNANCE_LOG_LEVEL=info
FYNANCE_DB_PATH=C:\Users\you\Documents\fynance.db
FYNANCE_ANTHROPIC_API_KEY=sk-ant-...
```
**Important:** the binary looks for `.env` in the folder you are *in* when you run it (your current working directory), not the folder the binary lives in. The simplest reliable setup is to keep a dedicated folder, put the `.env` file in it, and always start fynance from that folder (`cd` into it first, then run `fynance serve`). On Windows, note that File Explorer hides file extensions by default, so a file you name `.env` can silently become `.env.txt`, double-check via **View → File name extensions**.

**3. Real environment variables.** Set them in your shell or OS so they apply to every run.

Windows, for the current PowerShell window only:
```powershell
$env:FYNANCE_PORT = "8080"
fynance serve
```
Windows, permanently for your user account (set once, applies to every new terminal):
```powershell
[Environment]::SetEnvironmentVariable("FYNANCE_PORT", "8080", "User")
```
macOS / Linux, for the current terminal:
```bash
export FYNANCE_PORT=8080
fynance serve
```
macOS / Linux, permanently: add the same `export` line to your shell profile (`~/.zshrc` on macOS, `~/.bashrc` on most Linux).

> **Do I need an Anthropic key?** Only if you want to import bank statements *through the web UI or `fynance import`*, that feature uses Claude to read the statement. Browsing, budgeting, and viewing your portfolio do not need a key. If you import via an external agent instead, the key lives with that agent, not here. See the table below for `FYNANCE_ANTHROPIC_API_KEY` vs `FYNANCE_CLAUDE_CODE_OAUTH_TOKEN`.

### Environment Variables

The variables below can be set any of the three ways described in [Configuring the standalone binary](#configuring-the-standalone-binary) above. For local development from a cloned repo, the repo includes a `.env.example` with safe defaults, copy it to `.env` and fill in your values. The `.env` file is gitignored and should never be committed.

| Variable | Default | Required | Description |
|---|---|---|---|
| `FYNANCE_PORT` | `7433` | No | HTTP server port (serves both web UI and REST API) |
| `FYNANCE_HOST` | `127.0.0.1` | No | Bind address. Set to `0.0.0.0` in Docker so port mapping can reach the server |
| `FYNANCE_DB_PATH` | OS data dir | No | Full path to the SQLite database file. In Docker set to `/home/fynance/data/fynance.db` |
| `FYNANCE_LOG_LEVEL` | `info` | No | Log verbosity. Options: `trace`, `debug`, `info`, `warn`, `error` |
| `FYNANCE_ANTHROPIC_API_KEY` | (none) | One of these two | Console API key (`sk-ant-api...`) from [console.anthropic.com](https://console.anthropic.com/). Pay-per-token billing |
| `FYNANCE_CLAUDE_CODE_OAUTH_TOKEN` | (none) | One of these two | Claude Pro/Max subscription token (`sk-ant-oat01...`) from `claude setup-token`. Billed against your subscription's Agent SDK credit pool, not per-token |
| `FYNANCE_IMPORT_LLM_MODEL` | `claude-sonnet-4-6` | No | Claude model used by the CSV/statement parser (Haiku can be set to cut cost) |
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
fynance profile add --id <id> --name <name>  # create a profile (accounts need one)
fynance profile list
fynance profile delete <id>                  # refuses if any account still references it
fynance transaction delete <id>... [--account <id>]   # hard-delete txns by id, or all for an account
fynance budget set --month YYYY-MM --category <c> --amount N
fynance budget status --month YYYY-MM
fynance stats
fynance token create --name <name>           # generate API token for programmatic access
fynance token list
fynance token revoke --name <name>
fynance migrate-subunits [--apply]           # convert stored GBX/USX/ZAC/ILA rows to GBP/USD/ZAR/ILS
```

### Runbook: migrating broker sub-unit currencies

Some brokers quote in a **sub-unit** of a currency rather than the currency
itself — most commonly **GBX** ("pence sterling", 1/100 of a GBP), and likewise
USX, ZAC and ILA. Every write path now converts these to the parent currency at
import time, so new data is never stored as GBX. `fynance migrate-subunits`
exists to convert rows written *before* that behaviour landed, across
`investments`, `holdings`, `transactions` and `accounts`, and to then drop the
now-unreferenced sub-unit rows from the `currencies` table.

Run it like this:

```bash
fynance migrate-subunits            # dry run: prints every change, writes nothing
fynance migrate-subunits --apply    # writes the changes
```

**Two rules, in order:**

1. **Always dry-run first, and read the output.** The dry run prints the exact
   before/after for every row it would touch. `--apply` should only ever be run
   after you have looked at that list and it says what you expect.

2. **⚠️ Run `--apply` BEFORE deleting the sub-unit currency from Settings (or
   via any manual `currencies` delete) — never after.** This is the one ordering
   that can silently corrupt your figures. Conversion is driven by the fixed
   sub-unit table in code, but *other* read paths resolve a currency through the
   stored FX rate, and `FxRateMap::convert` is deliberately lenient: asked to
   convert a currency it has no rate for, it logs a warning and **returns the
   amount unchanged**. So if the `GBX` row is deleted while GBX-denominated rows
   still exist, `381600` stops being converted and a balance of **£3,816.00
   starts reading as £381,600** — a 100× overstatement, with nothing but a log
   line to say so. Migrating the rows first means there is nothing left
   referencing GBX, and the migration then removes that `currencies` row for you
   as its final step.

The migration is **idempotent** — it only ever matches rows whose currency is a
sub-unit code, so a second run finds nothing and is a no-op. It is also safely
**re-runnable after a failure**: each table is migrated in its own transaction,
so an error partway through leaves already-converted tables converted and the
rest untouched, and re-running finishes the job. It is *not* all-or-nothing
across tables — see `Db::migrate_subunit_currencies` for the precise contract.

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

**Terminal 1**, backend with live reload:
```bash
cd backend
cargo watch -x 'run -- serve --no-open'
```
The backend API starts on `http://localhost:7433` and auto-recompiles when `.rs` files change.

**Terminal 2**, frontend with hot module replacement:
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

Use this if you're only working on the Rust backend or want to test the embedded UI. Requires `frontend/dist` to already exist (see [Initial Setup](#initial-setup) above or the ⚠️ warning under [Build](#build) below) — the embed happens at compile time via `include_dir!`.

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
cargo build          # backend only -- requires frontend/dist to already exist, see warning below
cd frontend && npm run build   # frontend only
```

`make build` runs `npm run build` in the frontend folder then `cargo build --release` in the backend. The result is a single binary at `backend/target/release/fynance` with the compiled React app embedded.

> **⚠️ Any bare `cargo` command (`build`, `test`, `clippy`, `run`, `cargo watch`, …) requires `frontend/dist` to exist first.** `frontend/dist` is gitignored, so a fresh clone or worktree does not have it, and `cargo build` on its own will fail with a proc-macro panic from `include_dir!` rather than a normal compile error. Run `cd frontend && npm run build` (or `make build`) once before any bare `cargo` command. See `backend/RUNNING.md` → Troubleshooting for the exact error text and fix.

### Testing and Validation

PRs that fail CI will not be merged. Before pushing, run (after `cd frontend && npm run build` at least once — see warning above):

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
