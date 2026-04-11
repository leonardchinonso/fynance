# Architecture

## System Overview

fynance is a Rust CLI binary that acts as a pipeline: it reads bank statements in various formats, normalizes and categorizes the data, stores it in SQLite, and optionally writes Obsidian markdown reports. Obsidian's SQLite DB plugin then renders live SQL queries and charts inside notes.

## Component Diagram

```
┌─────────────────────────────────────────────────────────┐
│                    Input Sources                         │
│   [Chase CSV]  [BofA CSV]  [QFX files]  [Bank PDFs]     │
└──────────────────────┬──────────────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────────────┐
│                  Importer Layer                          │
│                                                         │
│  ┌──────────────┐ ┌──────────────┐ ┌────────────────┐  │
│  │ CsvImporter  │ │ OfxImporter  │ │  PdfImporter   │  │
│  │  (csv crate) │ │ (roxmltree)  │ │ (pdf-extract + │  │
│  │              │ │              │ │  Claude vision) │  │
│  └──────┬───────┘ └──────┬───────┘ └───────┬────────┘  │
│         └────────────────┼─────────────────┘           │
│                          │ Iterator<Item=Transaction>   │
│              [normalize_description()]                  │
│              [dedup: FITID or fingerprint hash]         │
└──────────────────────────┬──────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────┐
│               Categorization Pipeline                    │
│                                                         │
│  1. [Rule Engine]  -- confidence >= 0.85?               │
│         no  ↓                                           │
│  2. [Claude Haiku] -- confidence >= 0.75?               │
│         no  ↓                                           │
│  3. [Review Queue] -- user resolves                     │
│                                                         │
│  Bulk: Batch API (50% cheaper, async)                   │
│  Live: On-demand API (instant)                          │
└──────────────────────────┬──────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────┐
│                  SQLite Database                         │
│       (~/SecondBrain/financial/transactions.db)          │
│                                                         │
│  transactions  │  budgets  │  accounts  │  review_queue │
└──────────────────────────┬──────────────────────────────┘
                           │
              ┌────────────┼────────────┐
              ▼            ▼            ▼
┌─────────────────┐ ┌───────────┐ ┌────────────────────┐
│  Obsidian Notes │ │  CSV      │ │  Claude Sonnet     │
│  SQLite DB      │ │  export   │ │  Budget analysis,  │
│  plugin queries │ │           │ │  monthly insights  │
└─────────────────┘ └───────────┘ └────────────────────┘
```

## Module Dependency Graph

```
main.rs
  └── cli.rs (clap subcommands)
       ├── commands/import.rs
       │    ├── importers/csv_importer.rs
       │    ├── importers/ofx_importer.rs
       │    └── importers/pdf_importer.rs
       │         └── categorizer/claude.rs (vision fallback)
       ├── commands/categorize.rs
       │    └── categorizer/pipeline.rs
       │         ├── categorizer/rules.rs
       │         └── categorizer/claude.rs
       ├── commands/review.rs
       ├── commands/report.rs
       │    └── budget/advisor.rs
       │         └── categorizer/claude.rs
       └── commands/budget.rs
            └── budget/analyzer.rs

All commands depend on:
  model.rs       (Transaction, Category types)
  storage/db.rs  (Db struct, queries)
  config.rs      (load YAML configs)
```

## CLI Surface

```bash
# Import a single file (auto-detect format)
fynance import statement.csv
fynance import statement.qfx

# Import with explicit account mapping
fynance import ~/Downloads/chase-2024.csv --account chase-checking

# Batch import a directory
fynance import ~/Downloads/statements/

# Categorize all uncategorized transactions
fynance categorize              # on-demand, one by one
fynance categorize --batch      # submit Batch API job (async)
fynance categorize --check <id> # poll a running batch

# Interactive review of low-confidence items
fynance review

# Reports
fynance report --month 2026-04  # write monthly note to Obsidian vault
fynance report --year 2025

# Budget
fynance budget init --income 5200 --month 2026-05
fynance budget set --month 2026-05 --category "Food: Dining & Bars" --amount 250
fynance budget status           # current month variance table

# Quick terminal summary
fynance stats

# Export
fynance export --year 2025 --format csv
```

## Configuration (`config/accounts.yaml`)

```yaml
vault_path: ~/SecondBrain

accounts:
  - id: chase-checking
    bank: chase
    name: Chase Total Checking
    type: checking
    format: csv
    column_map:
      date: "Transaction Date"
      description: "Description"
      amount: "Amount"
      amount_sign: signed        # negative = debit, as-is

  - id: bofa-checking
    bank: bofa
    name: Bank of America Checking
    type: checking
    format: csv
    column_map:
      date: "Date"
      description: "Description"
      debit: "Debit Amount"
      credit: "Credit Amount"
      amount_sign: split         # separate columns

  - id: apple-card
    bank: apple
    name: Apple Card
    type: credit
    format: csv
    column_map:
      date: "Transaction Date"
      description: "Merchant"
      amount: "Amount (USD)"
      amount_sign: negate        # Apple shows positive for charges
```

## Design Principles

1. **No UI**: Obsidian is the UI. Rust does data processing; Obsidian renders.
2. **Incremental imports**: Deduplication (FITID + fingerprint) means re-importing the same file is always safe.
3. **Single binary**: `cargo build --release` produces one self-contained executable with SQLite bundled.
4. **Offline-first**: The only network calls are to the Claude API for categorization. Viewing data in Obsidian requires no connectivity.
5. **Cheap**: Entire 3-year historical setup costs ~$0.75 in API calls. Ongoing ~$0.05/month.
6. **Auditable**: Raw exports stored in `raw-exports/`, never modified. Every import logged to `import_log`.
