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
- **API-first, no internal AI for MVP**: the fynance binary makes zero outbound API calls. Categorization, data extraction (including from screenshots), and any AI processing are handled by external agents that push pre-processed data through the REST API. The API docs at `/api/docs` are designed to be usable as an AI agent system prompt.
- Obsidian integration via a well-documented read/write REST API (OpenAPI spec at `/api/docs`), plus markdown export endpoint for monthly summaries
- Shared GH repo, each contributor runs their own Docker instance (no shared DB)
- Fully local, privacy-first: no data leaves the machine. The binary has no outbound network calls.

## Open Questions
- CSV format detection should be dynamic: auto-detect bank and format version from column headers rather than requiring the user to specify. Support all known versions and gracefully handle unknown formats with clear error messages.
- **Budgets: standing vs per-month?** Current schema has per-month budgets (`UNIQUE(month, category)`). Proposal: budgets are standing targets (one per category), and you just query transactions for any month against the target. Per-month allows seasonal variation; standing is simpler. See `DISCUSS` comments in `docs/design/03_data_model.md`.
- **HoldingType::Cash**: Should uninvested cash in brokerage accounts be a holding or only tracked via account balance in `portfolio_snapshots`? See Open Questions in `docs/design/03_data_model.md`.
- **Holdings vs portfolio_snapshots consolidation**: Ope and Nonso both flagged overlap between the `holdings` and `portfolio_snapshots` tables. Should they be consolidated into a single table? Current design keeps them separate (holdings = per-symbol detail, snapshots = account-level balances), but this may be over-engineered for MVP. See `DISCUSS` in `docs/design/03_data_model.md`. (PR #1)
- **ingestion_checklist table: needed for MVP?** Nonso suggested starting small and adding tables as needed. The guided ingestion flow could potentially derive status from `import_log` instead of a dedicated table. See `DISCUSS` in `docs/design/03_data_model.md`. (PR #1)
- **Docker for MVP: needed now?** Nonso questioned whether Docker is needed at this stage, since the app runs locally and SQLite is just a file. Docker adds complexity; could defer to post-MVP. Currently documented in README and plan. (PR #1)
- **API-first vs internal AI**: Original plan (on master) had internal Claude API calls for categorization, screenshot extraction, and report generation. New proposal: defer all internal AI to post-MVP. The binary makes zero outbound calls. External AI agents handle categorization and data extraction, pushing results through the REST API. API docs at `/api/docs` are designed as an agent-readable system prompt. Internal AI becomes an optional V1 convenience layer. Needs Nonso's buy-in since this changes the Phase 5 architecture. (PR #1)

- **RSU / stock-denominated income**: Vested RSUs are income valued in shares, not currency. Options: (a) Transaction with category "Income: RSU Vesting" + a `unit` field (currency vs shares), or (b) separate `stock_income` table. Implications for the Transaction model. Currently RSUs are tracked as holdings snapshots only.
- **Liability account types**: Mortgage balance and credit card balance as negative-balance accounts. AccountType may need 'mortgage' or a general 'liability' type. Home equity = home value holding - mortgage account - HTB loan account. HoldingType may need 'property' for home value tracking.
- **Tax calcs for RSU forecasting**: Employer NI rate, tax rate, NI charge applied to gross RSU vesting to project net shares/value. Future forecasting endpoint.
- **Multi-profile data model**: Frontend currently adds a `profile_id` to Account. Need to decide if this is a first-class DB concept or handled in the application layer.

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
- 2026-04-11: API-first model -- deferred internal AI (Claude categorization, Vision screenshot extraction, AI reports) to post-MVP. External agents handle all AI processing via the REST API. API docs designed as agent-readable system prompt.
