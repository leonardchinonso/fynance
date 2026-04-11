# fynance

## Goal
Personal finance tracker for OJ and Nonso. Ingest bank CSVs, categorize spending, track budgets and net worth, all via a local web UI.

## Status
Active -- planning phase, no implementation yet.

## Key Decisions
- React 19 + React Compiler for frontend (no manual memoization)
- REST API architecture (not SSR, not RPC)
- Single Docker container for production deployment, SQLite on a volume
- Token-based auth for programmatic API access (agents, scripts)
- Screenshot ingestion via Claude Vision for low-friction data entry
- Obsidian-compatible markdown export for monthly summaries
- Shared GH repo, each contributor runs their own Docker instance (no shared DB)
- Fully local, privacy-first: no data leaves the machine except opt-in Claude API calls

## Open Questions
- Category taxonomy: finalize the two-level hierarchy for both users' spending patterns
- Which Monzo/Revolut CSV format versions to support (formats may change over time)

## Contributors
- OJ (Wunderkind): frontend focus, UI/UX requirements, Obsidian integration
- Nonso (Leonard): Rust backend, CLI, architecture, security

## Links
- Repo: this repo (fynance-be)
- Plans: plans/08_mvp_phases_v2.md
- Design docs: design/

## Future Roadmap (Post-MVP)
1. Tax planning for capital gains
2. Early retirement planning
3. Rental income tracking
4. Forecasting for big purchases
5. AI chat interface for querying finances

## Log
- 2026-04-11: Initial plan updates -- React 19, Docker deployment, .env config, API token auth, screenshot import, Obsidian export, future roadmap
