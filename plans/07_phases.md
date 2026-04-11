# Implementation Phases

## Phase 1: CSV Import Foundation

**Goal**: A working Rust binary that reads a CSV bank statement and stores transactions in SQLite. No AI, no LLM, no async, no other file formats.

### Scope constraints

- Input: CSV only. OFX, QFX, and PDF formats are out of scope for this phase.
- No categorization pipeline (no rule engine, no Claude API calls).
- No `tokio` or `reqwest` — no async needed without network calls.
- No `serde_yaml` — no YAML config parsing yet.
- The `SourceFormat` enum has only `Csv` for now.

### Dependencies (`Cargo.toml`)

Only include what Phase 1 actually needs:

- `clap` (derive) — CLI argument parsing
- `rusqlite` (bundled) — SQLite storage
- `serde`, `serde_json` — serialization
- `csv` — CSV parsing
- `chrono` — date handling
- `rust_decimal` — money math
- `uuid` — transaction ID generation
- `anyhow`, `thiserror` — error handling
- `tracing`, `tracing-subscriber` — logging
- `once_cell`, `regex`, `sha2`, `hex` — description normalization and fingerprinting
- `indicatif` — progress bar during import
- Dev: `tempfile`, `pretty_assertions`

### Tasks

- [ ] Initialize the Rust project (`cargo init`)

- [ ] Write `Cargo.toml` with Phase 1 dependencies only (see above)

- [ ] Write `sql/schema.sql` — `transactions` and `import_log` tables only

- [ ] Implement `src/model.rs`:
  - `Transaction` struct
  - `SourceFormat` enum (Csv only for now)

- [ ] Implement `src/util.rs`:
  - `normalize_description()`
  - `fingerprint()`
  - `parse_date()`

- [ ] Implement `src/storage/db.rs`:
  - `Db::open()` (WAL mode, runs schema.sql)
  - `insert_transaction()` returning `InsertResult` (Inserted | Duplicate)
  - `log_import()`

- [ ] Implement `src/importers/mod.rs`: `Importer` trait

- [ ] Implement `src/importers/csv_importer.rs`:
  - `BankMapping` / `AmountSign` types
  - `CsvImporter` with constructor for Chase format
  - Implement `Importer` trait

- [ ] Implement `src/commands/import.rs`: parse file, insert rows, print summary

- [ ] Implement `src/cli.rs` + `src/main.rs`:
  - `fynance import <file> --account <id>`
  - `fynance stats` (total count, date range, count per account)

- [ ] Test with a real CSV export
  - Verify row count matches source
  - Verify deduplication works (re-import same file)
  - Verify amounts have correct sign

**Deliverable**: `cargo run -- import statement.csv --account chase-checking` inserts transactions into SQLite.

---

## Phase 2: Multi-Bank Import + OFX (Week 2)

**Goal**: All accounts imported, 2-3 years of history in the database.

### Tasks

- [ ] Add BofA CSV importer (split debit/credit columns, 6 skip rows)
- [ ] Add Apple Card CSV importer (negate amounts)
- [ ] Add Wells Fargo CSV importer (positional columns, no header)
- [ ] Implement `src/importers/ofx_importer.rs` using `roxmltree`
  - SGML header stripping
  - FITID-based dedup

- [ ] Implement `src/importers/pdf_importer.rs`
  - `pdf-extract` + regex as primary
  - Claude vision as fallback (gate behind `--use-vision` flag)

- [ ] Implement `get_importer()` dispatcher in `src/importers/mod.rs`

- [ ] Extend CLI: `fynance import <dir>` for directory batch import

- [ ] Import all 2-3 years of statements
  - Verify totals against known balances
  - Resolve any duplicate issues

- [ ] Implement `fynance stats` output:
  - Total transactions, date range
  - Count per account
  - Count uncategorized

**Deliverable**: 2-3 years fully imported, deduplicated, queryable.

---

## Phase 3: Categorization (Week 3)

**Goal**: 90%+ of transactions categorized.

### Tasks

- [ ] Define category taxonomy in `config/categories.yaml`

- [ ] Write initial `config/rules.yaml` with patterns for your actual spending
  - Test coverage: run against all transactions, count rule hit rate

- [ ] Implement `src/categorizer/rules.rs`: YAML-driven rule loader + `match_rules()`

- [ ] Implement `src/categorizer/claude.rs`:
  - `categorize_one()` with prompt caching
  - `submit_batch()` and `fetch_batch_results()`
  - `CatResult` struct

- [ ] Implement `src/categorizer/pipeline.rs`:
  - `run_all()` with rule-first then Claude
  - Review queue population

- [ ] Extend CLI:
  ```bash
  fynance categorize             # on-demand
  fynance categorize --batch     # submit batch job
  fynance categorize --check <id> # collect batch results
  ```

- [ ] Run categorization on all imported transactions:
  ```bash
  fynance categorize --batch
  # Wait 5-30 minutes
  fynance categorize --check <batch_id>
  ```

- [ ] Implement `fynance review` interactive CLI

- [ ] Tune rules based on review queue patterns
  - Add missed merchants to `config/rules.yaml`
  - Re-run; review queue should shrink

**Deliverable**: 90%+ categorized. Review queue under 50 items.

---

## Phase 4: Obsidian Integration (Week 4)

**Goal**: Rich dashboard live in Obsidian.

### Tasks

- [ ] Create vault structure
  ```bash
  mkdir -p ~/SecondBrain/financial/{raw-exports,monthly,yearly,_templates}
  ```

- [ ] Place database in vault
  ```bash
  cp # or symlink transactions.db into ~/SecondBrain/financial/
  ```

- [ ] Install Obsidian plugins: Dataview, Templater, SQLite DB, Charts

- [ ] Configure SQLite DB plugin: `financial/transactions.db`

- [ ] Create `dashboard.md` with all queries (see `plans/05_obsidian_integration.md`)

- [ ] Create monthly template in `_templates/monthly.md`

- [ ] Implement `fynance report --month <YYYY-MM>` command:
  - Appends Claude analysis to existing monthly note

- [ ] Backfill last 6 monthly notes:
  ```bash
  fynance report --month 2025-10 --with-analysis
  # ... through 2026-03
  ```

- [ ] Create 2024 and 2025 yearly summary notes

**Deliverable**: Working Obsidian dashboard with real spending data and charts.

---

## Phase 5: Budgeting (Week 5)

**Goal**: First budget set, monthly workflow established.

### Tasks

- [ ] Implement `src/budget/analyzer.rs`: `project_next_month()`, `check_50_30_20()`

- [ ] Implement `src/budget/advisor.rs`: `generate_initial_budget()`, `monthly_insights()`

- [ ] Implement `src/storage/db.rs` budget methods:
  - `set_budget()`, `get_budgets()`, `budget_vs_actual()`, `category_averages_6mo()`

- [ ] Extend CLI:
  ```bash
  fynance budget init --income 5200 --month 2026-05
  fynance budget set --month 2026-05 --category "Food: Dining & Bars" --amount 250
  fynance budget status
  fynance budget analyze --month 2026-04
  ```

- [ ] Generate initial budget:
  ```bash
  fynance budget init --income <your_income> --month 2026-05
  ```

- [ ] Review and tweak generated budget

- [ ] Add budget vs actual to dashboard.md

- [ ] Establish monthly workflow:
  1. Download statements from bank websites
  2. `fynance import ~/Downloads/*.csv`
  3. `fynance categorize`
  4. Open Obsidian, create monthly note (Cmd+Shift+M)
  5. `fynance report --month YYYY-MM --with-analysis`
  6. Review next month's budget

**Deliverable**: First budget set, monthly cadence working end-to-end.

---

## Phase 6: Polish (Week 6+)

**Goal**: Minimal friction for ongoing monthly use.

### Tasks

- [ ] `fynance monthly` composite command (runs import, categorize, report in sequence)

- [ ] Tax category support: flag deductible expenses, export filtered CSV
  ```bash
  fynance export --year 2025 --category "Health: Medical & Dental" --format csv
  ```

- [ ] Annual review command:
  ```bash
  fynance report --year 2025
  ```

- [ ] Rule improvement loop: "fynance suggest-rules" asks Claude to analyze Other-categorized transactions and propose new patterns

- [ ] `--dry-run` flag on `import` to preview without writing

---

## First Thing to Build

Start with these files in order:

1. `Cargo.toml` with dependencies
2. `sql/schema.sql`
3. `src/model.rs`
4. `src/util.rs`
5. `src/storage/db.rs` with `open()` and `insert_transaction()`
6. `src/importers/csv_importer.rs` for Chase
7. `src/main.rs` + `src/cli.rs` with `import` and `stats` subcommands

Run `cargo run -- import statement.csv --account chase-checking` against a real export as the integration test. Everything else builds on having clean data in the database.

## Milestones Summary

| Week | Milestone |
|---|---|
| 1 | Chase CSV imported, data in SQLite |
| 2 | All banks + OFX/PDF imported, 2-3 years of history |
| 3 | All transactions categorized |
| 4 | Obsidian dashboard live |
| 5 | Budget set, monthly workflow established |
| 6+ | Polish, tax export, annual review |
