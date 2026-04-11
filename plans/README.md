# fynance Implementation Plan

A personal finance tracker written in Rust that ingests bank statements, categorizes transactions with Claude, stores data in SQLite, and surfaces insights through Obsidian.

## Plan Documents

| File | Contents |
|---|---|
| [01_architecture.md](01_architecture.md) | System design, component diagram, data flow |
| [02_data_model.md](02_data_model.md) | SQLite schema, Rust structs, category taxonomy |
| [03_importer.md](03_importer.md) | Statement parsing pipeline: CSV, OFX/QFX, PDF |
| [04_categorizer.md](04_categorizer.md) | Hybrid rule + Claude categorization system |
| [05_obsidian_integration.md](05_obsidian_integration.md) | Vault structure, plugin config, dashboard templates |
| [06_budgeting.md](06_budgeting.md) | Budget generation, variance tracking, Claude analysis |
| [07_phases.md](07_phases.md) | Phased implementation timeline with concrete tasks |

## Tech Stack Decision

| Layer | Choice | Reason |
|---|---|---|
| Language | Rust (edition 2024) | Performance, correctness, single-binary deploy |
| CLI | `clap` with derive | Standard, ergonomic |
| Storage | SQLite via `rusqlite` (bundled) | No system dependency, portable |
| Async runtime | `tokio` | Required for `reqwest` and concurrent Claude calls |
| HTTP | `reqwest` + `serde_json` | Call Claude API directly, no alpha SDKs |
| CSV | `csv` + `serde` | Mature, fast |
| OFX/QFX | `roxmltree` (+ manual SGML strip) | Rust OFX ecosystem is thin, keep it simple |
| PDF | `pdf-extract` + Claude vision fallback | Rust PDF table extraction is weaker than Python |
| Config | `serde_yaml` | Rules and categories live in editable YAML |
| Money | `rust_decimal::Decimal` | Never use f64 for currency |
| Error | `anyhow` at boundaries, `thiserror` in libs | Standard Rust pattern |

## Project Directory Structure

```
~/projects/fynance/
├── Cargo.toml
├── Cargo.lock
├── CLAUDE.md
├── README.md
├── src/
│   ├── main.rs                  # Entry point, tokio::main
│   ├── cli.rs                   # clap subcommand definitions
│   ├── model.rs                 # Transaction struct, Category enum
│   ├── config.rs                # Load YAML configs
│   ├── storage/
│   │   ├── mod.rs
│   │   ├── db.rs                # rusqlite wrapper
│   │   └── migrations.rs        # Schema creation and versioning
│   ├── importers/
│   │   ├── mod.rs               # Importer trait + dispatcher
│   │   ├── csv_importer.rs      # Bank-specific CSV parsers
│   │   ├── ofx_importer.rs      # OFX/QFX parser
│   │   └── pdf_importer.rs      # pdf-extract + Claude vision fallback
│   ├── categorizer/
│   │   ├── mod.rs
│   │   ├── rules.rs             # Regex rule engine
│   │   ├── claude.rs            # Claude API client and prompts
│   │   └── pipeline.rs          # Hybrid orchestrator
│   ├── budget/
│   │   ├── mod.rs
│   │   ├── analyzer.rs          # Trend analysis, projections
│   │   └── advisor.rs           # Budget recommendations via Claude
│   └── commands/
│       ├── mod.rs
│       ├── import.rs
│       ├── categorize.rs
│       ├── review.rs
│       ├── report.rs
│       └── budget.rs
├── sql/
│   └── schema.sql               # SQLite DDL
├── config/
│   ├── rules.yaml               # Categorization regex rules
│   ├── categories.yaml          # Category taxonomy
│   └── accounts.yaml            # Account definitions
├── obsidian/
│   ├── templates/
│   │   ├── monthly.md           # Templater monthly report template
│   │   └── dashboard.md
│   └── README.md                # Setup instructions
└── tests/
    ├── fixtures/                # Sample CSV, OFX, PDF files
    └── integration.rs
```
