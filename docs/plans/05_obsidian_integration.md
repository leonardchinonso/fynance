# Obsidian Integration [DROPPED]

> **This plan is no longer part of fynance.**
>
> Prompt 1.1 (2026-04-11) pivoted the project away from Obsidian as the UI layer in favor of a purpose-built React web UI served by a local Axum server. The Obsidian SQLite DB plugin is not used.
>
> This file is retained for historical context. Do not implement anything from it.

## What Replaced It

| Original Obsidian piece | New location |
|---|---|
| `~/SecondBrain/financial/transactions.db` | `~/.local/share/fynance/fynance.db` (per-OS-user) |
| `dashboard.md` with SQLite DB plugin | Transactions / Budget / Portfolio / Reports tabs in the React UI |
| Charts plugin bar / pie / line charts | Recharts components in React |
| Templater monthly template | `GET /api/reports/:month` rendered by the Reports page |
| `fynance report --month YYYY-MM` writing Markdown | `GET /api/reports/:month` returning JSON (Reports tab renders it) |

## Why the Pivot

1. **Portfolio overview requires cross-account rollups and diversity charts**. Obsidian's SQLite DB plugin renders single queries inline but does not support a rich dashboard layout with multiple interactive widgets.
2. **Per-user isolation** is a core requirement. The vault approach assumed a single user with a personal SecondBrain. The local web UI model scales to multiple OS users on the same machine.
3. **UI quality**. The new UI uses Recharts and shadcn-ui for polished visuals that the Charts plugin cannot match.
4. **Filtering and interaction**. Obsidian queries are static. The new UI has real filter controls, sorting, pagination, and inline edit.

## Historical Reference

The original vault layout, plugin list, and dashboard markdown lived here. They are preserved in git history prior to the Prompt 1.1 update. If the user ever wants a lightweight Obsidian export of monthly summaries alongside the web UI, that can be added as a `fynance export --format obsidian` subcommand later — but it is out of scope for the MVP.

See instead:

- `../design/01_ui_approaches.md` for why Axum + React was chosen
- `../design/02_architecture.md` for the new architecture
- `../design/04_portfolio_overview.md` for the portfolio view that replaced the Obsidian dashboard
- `08_mvp_phases_v2.md` for the current phased plan
