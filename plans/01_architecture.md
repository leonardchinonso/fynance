# Architecture

> **Updated after Prompt 1.1.** The original Obsidian-based architecture is kept in the git history; this file reflects the current plan. See `../design/02_architecture.md` for the full component diagram and module layout.

## System Overview

fynance is a single Rust binary that:

1. Runs a local-only Axum HTTP server on `127.0.0.1`
2. Serves a compiled React frontend embedded via `include_dir!`
3. Processes bank CSV imports (Monzo, Revolut, Lloyds) and stores transactions in SQLite
4. Categorizes transactions using a rules-first pipeline with Claude API fallback
5. Exposes four UI views: Transactions, Budget, Portfolio, Reports

The user runs `fynance serve`, the default browser opens, and all interaction happens in the browser. CLI subcommands remain available for automation.

## High-Level Component Diagram

```
┌─────────────────────────────────────────────────────────────┐
│                   fynance binary                             │
│                                                             │
│  ┌────────────┐    ┌──────────────────────────────────┐    │
│  │  CLI       │    │   Axum HTTP Server               │    │
│  │  (clap)    │    │   (127.0.0.1:PORT, loopback)     │    │
│  │            │    │                                  │    │
│  │  import    │    │   /api/transactions              │    │
│  │  serve     │    │   /api/budget/:month             │    │
│  │  account   │    │   /api/portfolio                 │    │
│  │  budget    │    │   /api/categorize                │    │
│  │  categorize│    │   /assets/* (embedded React)     │    │
│  └─────┬──────┘    └────────────────┬─────────────────┘    │
│        │                            │                       │
│        └──────────┬─────────────────┘                       │
│                   │                                          │
│           ┌───────▼────────┐                                │
│           │  Core Services  │                               │
│           │                │                                │
│           │  importers/    │                                │
│           │  categorizer/  │                                │
│           │  budget/       │                                │
│           │  portfolio/    │                                │
│           │  storage/      │                                │
│           └───────┬────────┘                                │
│                   │                                          │
│           ┌───────▼────────┐                                │
│           │  SQLite         │                               │
│           │  (per-user)     │                               │
│           └────────────────┘                                │
└─────────────────────────────────────────────────────────────┘
              ▲
              │ HTTP loopback
              │
       ┌──────┴──────┐
       │   Browser    │
       │   React UI   │
       └──────────────┘
```

## Module Dependency Graph

```
main.rs
  └── cli.rs (clap subcommands)
       ├── commands/serve.rs ──► server/ (Axum)
       │                           ├── routes/transactions.rs
       │                           ├── routes/budget.rs
       │                           ├── routes/portfolio.rs
       │                           ├── routes/import.rs
       │                           └── static_files.rs (include_dir!)
       ├── commands/import.rs ──► importers/
       │                            ├── csv_importer.rs
       │                            ├── monzo.rs
       │                            ├── revolut.rs
       │                            └── lloyds.rs
       ├── commands/categorize.rs ──► categorizer/
       │                                ├── rules.rs
       │                                ├── claude.rs
       │                                └── pipeline.rs
       ├── commands/account.rs ──► portfolio/
       │                             ├── accounts.rs
       │                             └── diversity.rs
       └── commands/budget.rs ──► budget/
                                    ├── analyzer.rs
                                    └── advisor.rs

All modules depend on:
  model.rs       (Transaction, Account, Budget, etc.)
  storage/db.rs  (Db, all SQL)
  util.rs        (normalize_description, fingerprint, parse_date)
```

## CLI Surface

```bash
# Start the web UI (primary workflow)
fynance serve [--port 3000] [--no-open]

# Data ingestion
fynance import <file|dir> --account <id>

# Account management
fynance account add --id <id> --name <name> --institution <inst> --type <type>
fynance account set-balance <id> <amount> --date YYYY-MM-DD
fynance account list

# Categorization
fynance categorize [--batch]

# Budget management
fynance budget set --month YYYY-MM --category <c> --amount N
fynance budget status

# Utilities
fynance stats
fynance export --year YYYY --format csv
fynance monthly    # composite: import + categorize + snapshot
```

## Storage Location

SQLite database is per-OS-user, resolved via the `dirs` crate:

| Platform | Path |
|---|---|
| macOS | `~/Library/Application Support/fynance/fynance.db` |
| Linux | `~/.local/share/fynance/fynance.db` |
| Windows | `%APPDATA%\fynance\fynance.db` |

Data directory is created with mode `0o700`; DB file with `0o600` on Unix. No shared storage, no centralized server. Each OS user runs their own isolated instance.

## Design Principles

1. **Browser is the UI**. Rust handles data and API; React handles presentation and charts.
2. **Loopback only**. The Axum server binds to `127.0.0.1` and never `0.0.0.0`. No LAN exposure, no auth needed.
3. **Single binary**. `cargo build --release` produces one executable with SQLite bundled and React bundle embedded.
4. **Per-user isolation**. DB path resolves from OS user home directory; file permissions restrict access.
5. **Incremental imports**. Deduplication by fingerprint hash means re-importing is always safe.
6. **Offline-first**. The UI works fully offline. Claude API calls are opt-in and only for categorization and monthly analysis.
7. **Auditable**. Every import logged to `import_log`.
8. **Money safety**. Decimals stored as TEXT in SQLite, parsed to `rust_decimal::Decimal` in Rust. Never `f32`/`f64`.
