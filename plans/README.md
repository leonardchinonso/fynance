# fynance Implementation Plan

A personal finance tracker written in Rust that ingests bank CSV statements, stores transactions in SQLite, and surfaces insights through Obsidian.

## Plan Documents

| File | Contents |
|---|---|
| [01_architecture.md](01_architecture.md) | System design, component diagram, data flow |
| [02_data_model.md](02_data_model.md) | SQLite schema, Rust structs |
| [03_importer.md](03_importer.md) | CSV parsing pipeline |
| [04_categorizer.md](04_categorizer.md) | Categorization (deferred) |
| [05_obsidian_integration.md](05_obsidian_integration.md) | Vault structure, plugin config, dashboard templates |
| [06_budgeting.md](06_budgeting.md) | Budget tracking (deferred) |
| [07_phases.md](07_phases.md) | Phased implementation timeline with concrete tasks |

## Tech Stack

| Layer | Choice | Reason |
|---|---|---|
| Language | Rust (edition 2024, MSRV 1.85) | Performance, correctness, single-binary deploy |
| CLI | `clap` with derive | Standard, ergonomic |
| Storage | SQLite via `rusqlite` (bundled) | No system dependency, portable |
| CSV | `csv` + `serde` | Mature, fast |
| Money | `rust_decimal::Decimal` | Never use f64 for currency |
| Error | `anyhow` at boundaries, `thiserror` in libs | Standard Rust pattern |

No async runtime or HTTP client until categorization is added.

## Project Directory Structure

```
~/projects/fynance/
├── Cargo.toml
├── Cargo.lock
├── CLAUDE.md
├── src/
│   ├── main.rs                  # Entry point
│   ├── cli.rs                   # clap subcommand definitions
│   ├── model.rs                 # Transaction struct, SourceFormat enum
│   ├── util.rs                  # normalize_description, fingerprint, parse_date
│   ├── storage/
│   │   ├── mod.rs
│   │   └── db.rs                # rusqlite wrapper
│   ├── importers/
│   │   ├── mod.rs               # Importer trait + dispatcher
│   │   └── csv_importer.rs      # Bank-specific CSV parsers
│   └── commands/
│       ├── mod.rs
│       ├── import.rs
│       └── stats.rs
├── sql/
│   └── schema.sql               # SQLite DDL
└── tests/
    ├── fixtures/                # Sample CSV files
    └── integration.rs
```
