# fynance Implementation Plan

A personal finance tracker written in Rust with a local React web UI. Ingests bank CSV statements, categorizes transactions, stores everything in a per-user SQLite database, and serves a browser UI via a loopback-only Axum server.

**The scope changed after Prompt 1.1**: Obsidian integration is dropped in favor of a purpose-built UI, and portfolio tracking is added. See `../design/` for the updated architecture rationale. V0 shipped; current work is on V1 features (CGT engine, multi-currency frontend) and the post-V0 roadmap.

## Active Plans

| # | File | Contents |
|---|---|---|
| 20 | [20_post_v0_plans.md](20_post_v0_plans.md) | Post-V0 roadmap (V1, V2, V3+) and unversioned ideas. Start here. |
| 22 | [22_multi_currency.md](22_multi_currency.md) | Multi-currency support spec. Backend shipped; frontend pending. |
| 23 | [23_capital_gains_v1.md](23_capital_gains_v1.md) | UK CGT engine V1: backend shipped, V1 finishing work in progress. |
| 24 | [24_streaming_progress_errors.md](24_streaming_progress_errors.md) | Streaming Anthropic API, SSE parse progress, and classified provider errors. |
| 25 | [25_import_gaps.md](25_import_gaps.md) | Multi-institution unified import + dryrun preview: gaps analysis and implementation plan. |

## Archived Plans

Closed, superseded, and dropped plans live in [`archive/`](archive/). They are kept for historical context and to preserve cross-references from in-flight work, but no new implementation should be derived from them.

| # | File | Contents | Status |
|---|---|---|---|
| 01 | [01_architecture.md](archive/01_architecture.md) | Axum + React system architecture, module graph, CLI surface | **Closed** (built) |
| 02 | [02_data_model.md](archive/02_data_model.md) | Rust types, full SQLite schema, queries | **Closed** (built, evolved via migrations) |
| 03 | [03_importer.md](archive/03_importer.md) | Monzo / Revolut / Lloyds CSV importer | **Superseded** by archive/10_llm_csv_import.md |
| 04 | [04_categorizer.md](archive/04_categorizer.md) | Rules + Claude pipeline, taxonomy, data minimization | **Deferred** (external agents handle categorization for MVP) |
| 05 | [05_obsidian_integration.md](archive/05_obsidian_integration.md) | Obsidian setup | **Dropped** |
| 06 | [06_budgeting.md](archive/06_budgeting.md) | Budget engine, queries, API, UI layout | **Closed** (built) |
| 07 | [07_phases.md](archive/07_phases.md) | Original CLI + Obsidian phased plan | **Superseded** by archive/08_mvp_phases_v2.md |
| 08 | [08_mvp_phases_v2.md](archive/08_mvp_phases_v2.md) | MVP phased plan (Axum + React) | **Closed** (MVP shipped; remaining items carried forward to 19 → 22, 23) |
| 09 | [09_backend_implementation_plan.md](archive/09_backend_implementation_plan.md) | Backend MVP executable checklist | **Closed** (phases 1-2 built, 3-6 superseded by 12) |
| 10 | [10_llm_csv_import.md](archive/10_llm_csv_import.md) | LLM-based CSV import design | **Closed** (built, replaces bank-specific parsers) |
| 11 | [11_frontend_backend_handover.md](archive/11_frontend_backend_handover.md) | Full API and model contract between frontend and backend | **Closed** (audited into 13) |
| 12 | [12_frontend_backend_consolidation.md](archive/12_frontend_backend_consolidation.md) | Integrate frontend handover requirements into backend phases 3-6 | **Closed** (BE built) |
| 13 | [13_frontend_backend_handover_unimplemented.md](archive/13_frontend_backend_handover_unimplemented.md) | Audit of 11: which handover asks are not yet built | **Closed** (remaining items in 19 → 22) |
| 14 | [14_holdings_consolidation_implementation.md](archive/14_holdings_consolidation_implementation.md) | Consolidate portfolio_snapshots into holdings | **Closed** (built, portfolio_snapshots dropped) |
| 15 | [15_portfolio_holdings_breakdown.md](archive/15_portfolio_holdings_breakdown.md) | Deep-dive on portfolio and holdings architecture | Reference |
| 16 | [16_fingerprint_and_snapshot_improvements.md](archive/16_fingerprint_and_snapshot_improvements.md) | Datetime-level granularity for fingerprints and snapshots | **Closed** (built, migrations applied) |
| 17 | [17_frontend_review.md](archive/17_frontend_review.md) | Frontend review: UX bugs and missing flows | **Closed** (bug fixed, account creation UI in 19, CORS in 20) |
| 18 | [18_project_brief.md](archive/18_project_brief.md) | Project goals, key decisions, open questions | **Closed** (V0 shipped; remaining open questions superseded by 20) |
| 19 | [19_v0_burndown.md](archive/19_v0_burndown.md) | V0 burndown: everything needed to ship | **Closed** (V0 shipped) |
| 21 | [21_capital_gains_tax.md](archive/21_capital_gains_tax.md) | UK CGT design rationale and HMRC background | **Closed** (V1 engine shipped; superseded by 23 for implementation tracking) |

## Tech Stack

| Layer | Choice | Reason |
|---|---|---|
| Language | Rust (edition 2024, MSRV 1.85) | Performance, correctness, single-binary deploy |
| CLI | `clap` with derive | Standard, ergonomic |
| Web server | `axum` on `tokio`, bound to `127.0.0.1` only | Single binary, local-only, no auth needed |
| Frontend | React 19 + Vite + TypeScript + Tailwind + shadcn-ui + Recharts, embedded via `include_dir!` | Best-in-class charts and UX for MVP |
| Storage | SQLite via `rusqlite` (bundled) at `dirs::data_local_dir()/fynance/fynance.db` | Per-OS-user isolation |
| AI | External agents handle categorization; push pre-processed data through REST API | See `04_categorizer.md` |
| CSV | `csv` + `serde` | Mature, fast |
| Money | `rust_decimal::Decimal` stored as SQLite TEXT | Never `f32`/`f64` |
| Error | `anyhow` at boundaries, `thiserror` in libs | Standard Rust pattern |

## Project Directory Structure

```
fynance/
├── CLAUDE.md
├── Makefile                     # build frontend then cargo
├── assets/
├── db/
│   └── sql/
│       ├── schema.sql           # SQLite DDL
│       └── migrations/
├── backend/
│   ├── Cargo.toml
│   ├── Cargo.lock
│   ├── config/
│   │   └── categories.yaml
│   └── src/
│       ├── main.rs
│       ├── lib.rs
│       ├── cli.rs
│       ├── model.rs
│       ├── util.rs
│       ├── storage/
│       ├── importers/
│       ├── server/
│       └── commands/
├── frontend/                    # React + Vite + TS
│   ├── package.json
│   ├── vite.config.ts
│   └── src/
│       ├── main.tsx
│       ├── App.tsx
│       ├── pages/
│       ├── components/
│       ├── api/
│       ├── bindings/            # ts-rs generated types
│       ├── context/
│       ├── data/                # mock data (removed when real API wired up)
│       ├── hooks/
│       ├── lib/
│       └── types/
├── docs/
│   ├── design/
│   ├── plans/                   # This folder
│   └── research/
└── .github/workflows/
```
