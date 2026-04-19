# fynance Implementation Plan

A personal finance tracker written in Rust with a local React web UI. Ingests bank CSV statements, categorizes transactions, stores everything in a per-user SQLite database, and serves a browser UI via a loopback-only Axum server.

**The scope changed after Prompt 1.1**: Obsidian integration is dropped in favor of a purpose-built UI, and portfolio tracking is added. See `../design/` for the updated architecture rationale. Current active work is tracked in `15_v0_burndown.md`.

## Plan Documents

| # | File | Contents | Status |
|---|---|---|---|
| 01 | [01_architecture.md](01_architecture.md) | Axum + React system architecture, module graph, CLI surface | **Closed** (built) |
| 02 | [02_data_model.md](02_data_model.md) | Rust types, full SQLite schema, queries | **Closed** (built, evolved via migrations) |
| 03 | [03_importer.md](03_importer.md) | Monzo / Revolut / Lloyds CSV importer | **Superseded** by `10_llm_csv_import.md` |
| 04 | [04_categorizer.md](04_categorizer.md) | Rules + Claude pipeline, taxonomy, data minimization | **Deferred** (external agents handle categorization for MVP) |
| 05 | [05_obsidian_integration.md](05_obsidian_integration.md) | Obsidian setup | **Dropped** |
| 06 | [06_budgeting.md](06_budgeting.md) | Budget engine, queries, API, UI layout | **Closed** (built: standing budgets, overrides, spending grid) |
| 07 | [07_phases.md](07_phases.md) | Original CLI + Obsidian phased plan | **Superseded** by `08_mvp_phases_v2.md` |
| 08 | [08_mvp_phases_v2.md](08_mvp_phases_v2.md) | Phased plan (Axum + React) | **Closed** (remaining items carried forward to 15) |
| 09 | [09_backend_implementation_plan.md](09_backend_implementation_plan.md) | Backend MVP executable checklist | **Closed** (phases 1-2 built, 3-6 superseded by 11) |
| 10 | [10_llm_csv_import.md](10_llm_csv_import.md) | LLM-based CSV import design | **Closed** (built, replaces bank-specific parsers) |
| 11 | [11_frontend_backend_consolidation.md](11_frontend_backend_consolidation.md) | Integrate frontend handover requirements into backend phases 3-6 | **Closed** (BE built, remaining items in 15 and 17) |
| 12 | [12_fingerprint_and_snapshot_improvements.md](12_fingerprint_and_snapshot_improvements.md) | Datetime-level granularity for fingerprints and snapshots | **Closed** (built, migrations applied) |
| 13 | [13_frontend_backend_handover_unimplemented.md](13_frontend_backend_handover_unimplemented.md) | Audit of 18: which handover asks are not yet built | **Closed** (remaining items carried forward to 15) |
| 14 | [14_holdings_consolidation_implementation.md](14_holdings_consolidation_implementation.md) | Consolidate portfolio_snapshots into holdings | **Closed** (built, portfolio_snapshots dropped) |
| 15 | [15_v0_burndown.md](15_v0_burndown.md) | V0 burndown: everything needed to ship | **Active** (start here) |
| 16 | [16_project_brief.md](16_project_brief.md) | Project goals, key decisions, open questions | Reference |
| 17 | [17_future_plans.md](17_future_plans.md) | Post-V0 roadmap (V1, V2, V3+) and unversioned ideas | Reference |
| 18 | [18_frontend_backend_handover.md](18_frontend_backend_handover.md) | Full API and model contract between frontend and backend (created Apr 12, predates 13) | **Closed** (audited into 13, remaining items in 15) |
| 19 | [19_portfolio_holdings_breakdown.md](19_portfolio_holdings_breakdown.md) | Deep-dive on portfolio and holdings architecture | Reference |
| 20 | [20_frontend_review.md](20_frontend_review.md) | Frontend review: UX bugs and missing flows | **Closed** (bug fixed, account creation UI in 15, CORS in 17) |

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
