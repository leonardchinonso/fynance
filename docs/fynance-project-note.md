# fynance

## Goal
Personal finance tracker. Ingest bank CSVs, categorize spending, track budgets and net worth, all via a local web UI.

## Status
Active -- planning phase, no implementation yet.

## Key Decisions
- React 19 + React Compiler for frontend (no manual memoization)
- REST API architecture (not SSR, not RPC)
- Single Docker container for production deployment, SQLite on a mounted Docker volume
- Token-based auth for programmatic API access (agents, scripts)
- Screenshot ingestion via Claude Vision for low-friction data entry
- Obsidian integration via a well-documented read/write REST API (OpenAPI spec at `/api/docs`), plus markdown export endpoint for monthly summaries
- Shared GH repo, each contributor runs their own Docker instance (no shared DB)
- Fully local, privacy-first: no data leaves the machine except opt-in Claude API calls

## Open Questions
- CSV format detection should be dynamic: auto-detect bank and format version from column headers rather than requiring the user to specify. Support all known versions and gracefully handle unknown formats with clear error messages.
- **Budgets: standing vs per-month?** Current schema has per-month budgets (`UNIQUE(month, category)`). Proposal: budgets are standing targets (one per category), and you just query transactions for any month against the target. Per-month allows seasonal variation; standing is simpler. See `DISCUSS` comments in `docs/design/03_data_model.md`.
- **HoldingType::Cash**: Should uninvested cash in brokerage accounts be a holding or only tracked via account balance in `portfolio_snapshots`? See Open Questions in `docs/design/03_data_model.md`.

## Contributors
- Ope (Zaida-3dO): frontend focus, UI/UX requirements, Obsidian integration
- Nonso (leonardchinonso): Rust backend, CLI, architecture, security

## Links
- Repo: this repo (fynance-be)
- Plans: docs/plans/08_mvp_phases_v2.md
- Design docs: docs/design/

## Future Roadmap (Post-MVP)
1. Tax planning for capital gains
2. Early retirement planning
3. Rental income tracking
4. Forecasting for big purchases
5. AI chat interface for querying finances

## Log
- 2026-04-11: Initial plan updates -- React 19, Docker deployment, .env config, API token auth, screenshot import, Obsidian export, future roadmap
