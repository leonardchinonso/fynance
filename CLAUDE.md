# fynance — Claude Context

Before starting any work, read ~/SecondBrain/02-projects/fynance.md for the goal, decisions, and open questions behind this project.

## Overview

Personal finance tracker that ingests bank statements (CSV, OFX/QFX, PDF), categorizes transactions with the Claude API, stores data in SQLite, and surfaces insights through the existing Obsidian vault at ~/SecondBrain.

Research lives in `./research/` and implementation plans live in `./plans/`. When picking up work, start at `plans/07_phases.md` Phase 1.

## Tech Stack

- **Language**: Rust (edition 2024, MSRV 1.85)
- **CLI**: `clap` with derive macros
- **Storage**: SQLite via `rusqlite` at `~/SecondBrain/financial/transactions.db`
- **AI**: Claude API via `reqwest` + `serde_json` (Haiku for categorization, Sonnet for analysis and PDF vision fallback)
- **Config**: YAML via `serde_yaml` for rules, categories, and accounts
- **Async**: `tokio` runtime (needed for concurrent HTTP calls to Claude)
- **Interface**: Terminal (CLI binary) + Obsidian notes (SQLite DB plugin renders SQL inline)

## Key Crates

| Crate | Purpose |
|---|---|
| `clap` | CLI argument parsing (derive) |
| `rusqlite` | SQLite driver with bundled feature |
| `reqwest` | HTTP client for Claude API |
| `serde`, `serde_json`, `serde_yaml` | Serialization and config |
| `tokio` | Async runtime |
| `csv` | CSV statement parsing |
| `pdf-extract` | PDF text extraction primary |
| `lopdf` | PDF lower-level table extraction fallback |
| `regex` | Rule-based categorization patterns |
| `chrono` | Date handling |
| `rust_decimal` | Precise money math, never f64 |
| `anyhow` | Application error context |
| `thiserror` | Library error enums |
| `tracing`, `tracing-subscriber` | Structured logging |
| `indicatif` | CLI progress bars for bulk imports |

## Commands

- Build: `cargo build --release`
- Run: `cargo run --release -- <subcommand>`
- Test: `cargo test`
- Lint: `cargo clippy --all-targets -- -D warnings`
- Format: `cargo fmt`
- Binary: `./target/release/fynance`

## CLI Subcommands

```bash
fynance import <file|dir> --account <id>    # Import statements
fynance categorize [--batch]                 # Run categorization pipeline
fynance review                               # Interactive review of low-confidence
fynance report --month YYYY-MM               # Generate monthly Obsidian note
fynance budget init --income N --month YYYY-MM
fynance budget status
fynance stats
fynance export --year YYYY --format csv
```

## Conventions

- Filenames use underscores, not spaces
- Avoid em dashes in any written content; use commas, colons, or separate sentences instead
- Money values use `rust_decimal::Decimal`, never `f32` or `f64`
- Dates stored as ISO 8601 strings (`YYYY-MM-DD`) in SQLite for portability and query simplicity
- Return `anyhow::Result<T>` at command boundaries, define `thiserror` enums inside modules
- Every importer must deduplicate: by OFX `FITID` when available, otherwise by a stable fingerprint hash
- Never log raw transaction descriptions at INFO level (may leak merchant info)
- Claude API calls always go through the `categorizer::claude` module so prompt caching and batch behavior stay consistent
