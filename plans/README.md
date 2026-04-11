# fynance Implementation Plan

A personal finance tracker written in Rust with a local React web UI. Ingests bank CSV statements, categorizes transactions, stores everything in a per-user SQLite database, and serves a browser UI via a loopback-only Axum server.

**The scope changed after Prompt 1.1**: Obsidian integration is dropped in favor of a purpose-built UI, and portfolio tracking is added. See `../design/` for the updated architecture rationale, and start at `08_mvp_phases_v2.md` when picking up work.

## Plan Documents

| File | Contents | Status |
|---|---|---|
| [01_architecture.md](01_architecture.md) | Axum + React system architecture, module graph, CLI surface | Active |
| [02_data_model.md](02_data_model.md) | Rust types, full SQLite schema, queries | Active |
| [03_importer.md](03_importer.md) | Monzo / Revolut / Lloyds CSV importer | Active |
| [04_categorizer.md](04_categorizer.md) | Rules + Claude pipeline, taxonomy, data minimization | Active |
| [05_obsidian_integration.md](05_obsidian_integration.md) | Obsidian setup | **DROPPED** (historical only) |
| [06_budgeting.md](06_budgeting.md) | Budget engine, queries, API, UI layout | Active |
| [07_phases.md](07_phases.md) | Original CLI + Obsidian phased plan | **SUPERSEDED** by `08_mvp_phases_v2.md` |
| [08_mvp_phases_v2.md](08_mvp_phases_v2.md) | **Current phased plan (Axum + React)** | Active (start here) |

## Tech Stack

| Layer | Choice | Reason |
|---|---|---|
| Language | Rust (edition 2024, MSRV 1.85) | Performance, correctness, single-binary deploy |
| CLI | `clap` with derive | Standard, ergonomic |
| Web server | `axum` on `tokio`, bound to `127.0.0.1` only | Single binary, local-only, no auth needed |
| Frontend | React 18 + Vite + TypeScript + Tailwind + shadcn-ui + Recharts, embedded via `include_dir!` | Best-in-class charts and UX for MVP |
| Storage | SQLite via `rusqlite` (bundled) at `dirs::data_local_dir()/fynance/fynance.db` | Per-OS-user isolation |
| AI | Claude API (Haiku for categorization, Sonnet for analysis) | See `04_categorizer.md` |
| CSV | `csv` + `serde` | Mature, fast |
| Money | `rust_decimal::Decimal` stored as SQLite TEXT | Never `f32`/`f64` |
| Error | `anyhow` at boundaries, `thiserror` in libs | Standard Rust pattern |

## Project Directory Structure

```
~/projects/fynance/
├── Cargo.toml
├── Cargo.lock
├── CLAUDE.md
├── Makefile                     # build frontend then cargo
├── sql/
│   └── schema.sql               # SQLite DDL
├── config/
│   ├── categories.yaml
│   └── rules.yaml
├── design/                      # Prompt 1.1 design docs (see design/README.md)
├── research/                    # Prompt 1 research artifacts
├── plans/                       # This folder
├── src/
│   ├── main.rs
│   ├── cli.rs                   # clap subcommand definitions
│   ├── model.rs                 # Transaction, Account, Budget, etc.
│   ├── util.rs                  # normalize_description, fingerprint, parse_date
│   ├── storage/
│   │   ├── mod.rs
│   │   └── db.rs
│   ├── importers/
│   │   ├── mod.rs               # Importer trait + dispatcher
│   │   └── csv_importer.rs      # Monzo / Revolut / Lloyds mappings
│   ├── categorizer/
│   │   ├── mod.rs
│   │   ├── rules.rs
│   │   ├── claude.rs
│   │   └── pipeline.rs
│   ├── budget/
│   │   ├── mod.rs
│   │   ├── analyzer.rs
│   │   └── advisor.rs
│   ├── portfolio/
│   │   ├── mod.rs
│   │   ├── accounts.rs
│   │   └── diversity.rs
│   ├── server/
│   │   ├── mod.rs               # Axum router, loopback binding, CORS
│   │   ├── routes/
│   │   │   ├── transactions.rs
│   │   │   ├── budget.rs
│   │   │   ├── portfolio.rs
│   │   │   └── import.rs
│   │   └── static_files.rs      # include_dir! embedded frontend
│   └── commands/
│       ├── mod.rs
│       ├── import.rs
│       ├── serve.rs
│       ├── categorize.rs
│       ├── account.rs
│       ├── budget.rs
│       └── stats.rs
├── frontend/                    # React + Vite + TS
│   ├── package.json
│   ├── src/
│   │   ├── main.tsx
│   │   ├── App.tsx
│   │   ├── pages/               # Transactions, Budget, Portfolio, Reports
│   │   ├── components/
│   │   └── api/                 # fetch wrappers
│   └── dist/                    # built output, embedded by Rust
└── tests/
    ├── fixtures/                # Sample CSV files
    └── integration.rs
```
