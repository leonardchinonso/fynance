# Architecture

## System Overview

fynance is a Rust CLI binary that reads bank statements in CSV format, normalizes the data, stores it in SQLite, and surfaces insights through the existing Obsidian vault at ~/SecondBrain. Obsidian's SQLite DB plugin renders live SQL queries and charts inside notes.

## Component Diagram

```
┌─────────────────────────────────────────────────────────┐
│                    Input Sources                         │
│                     [Bank CSVs]                          │
└──────────────────────┬──────────────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────────────┐
│                  Importer Layer                          │
│                                                         │
│               ┌──────────────┐                          │
│               │ CsvImporter  │                          │
│               │  (csv crate) │                          │
│               └──────┬───────┘                          │
│                      │ Iterator<Item=Transaction>        │
│            [normalize_description()]                     │
│            [dedup: fingerprint hash]                     │
└──────────────────────┬──────────────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────────────┐
│                  SQLite Database                         │
│       (~/SecondBrain/financial/transactions.db)          │
│                                                         │
│          transactions  │  import_log                    │
└──────────────────────────────────────────────────────────┘
                       │
              ┌────────┴────────┐
              ▼                 ▼
┌─────────────────┐      ┌───────────┐
│  Obsidian Notes │      │  CSV      │
│  SQLite DB      │      │  export   │
│  plugin queries │      │           │
└─────────────────┘      └───────────┘
```

## Module Dependency Graph

```
main.rs
  └── cli.rs (clap subcommands)
       ├── commands/import.rs
       │    └── importers/csv_importer.rs
       └── commands/stats.rs

All commands depend on:
  model.rs       (Transaction, SourceFormat types)
  storage/db.rs  (Db struct, queries)
  util.rs        (normalize_description, fingerprint, parse_date)
```

## CLI Surface

```bash
# Import a single CSV file with an account ID
fynance import statement.csv --account chase-checking

# Batch import a directory of CSVs
fynance import ~/Downloads/statements/

# Quick terminal summary
fynance stats
```

## Configuration

Account mappings are hardcoded in `src/importers/csv_importer.rs` for now via named constructors (`CsvImporter::chase`, etc.). YAML-based config is deferred to a later phase.

## Design Principles

1. **No UI**: Obsidian is the UI. Rust does data processing; Obsidian renders.
2. **Incremental imports**: Deduplication by fingerprint hash means re-importing the same file is always safe.
3. **Single binary**: `cargo build --release` produces one self-contained executable with SQLite bundled.
4. **Offline-first**: No network calls. All data lives locally.
5. **Auditable**: Every import logged to `import_log`.
