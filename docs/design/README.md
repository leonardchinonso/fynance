# Design Documents

These documents reflect the requirements from Prompt 1.1 — a rethink of the original CLI + Obsidian approach in favor of a proper local UI with portfolio tracking and multi-user isolation.

| Document | Topic |
|---|---|
| [01_ui_approaches.md](01_ui_approaches.md) | UI framework comparison: Tauri vs local web server vs egui vs Dioxus |
| [02_architecture.md](02_architecture.md) | Updated system architecture for the new requirements |
| [03_data_model.md](03_data_model.md) | Data model covering transactions, budgets, and portfolio accounts |
| [04_portfolio_overview.md](04_portfolio_overview.md) | Portfolio and diversity view design |
| [05_security_isolation.md](05_security_isolation.md) | Multi-user local isolation model |
| [06_entity_relations.md](06_entity_relations.md) | Entity relationship diagrams for all data models |

## Requirements (Prompt 1.1)

1. Ingest spending from CSV exports (Monzo, Revolut, Lloyds) and assign categories
2. Budget tab: spending per month per category, filterable
3. Portfolio overview: balances across accounts, diversity breakdown by type/sector
4. Good UI: optimized for user experience and visuals
5. Security: fully local storage per user; each user starts the service with a single command
6. Backend in Rust
7. MVP that does not make significant performance or usability sacrifices

## Recommended MVP Stack

**Backend**: Rust (Axum web server, SQLite via rusqlite)
**Frontend**: React + Vite + Recharts / shadcn-ui
**Launch**: `fynance serve` starts the Axum server; browser opens automatically to `localhost:PORT`
**Storage**: `~/.local/share/fynance/fynance.db` (per OS user, never shared)

See [01_ui_approaches.md](01_ui_approaches.md) for full comparison and rationale.
