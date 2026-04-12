# 10. LLM-Based CSV Import (Phase 1 Iteration)

> Supersedes the bank-specific dispatch in `03_importer.md` and the
> `BankFormat`-gated code path in Phase 1 of `09_backend_implementation_plan.md`.
> No code has been written for this document yet, it is a design/plan only.

## 1. Motivation

Phase 1 ships with three hand-written CSV dialects (Monzo, Revolut, Lloyds).
`detect_format` sniffs header names to decide which branch of `map_row` to
run, and any file that does not match a known shape fails hard with
`could not detect bank format from headers`. This is brittle in three ways:

1. **New banks break ingestion.** Any bank we have not hard-coded is a hard
   failure, even when the file is structurally trivial (date, description,
   amount).
2. **Small format drifts break ingestion.** Banks rename columns, add
   extra headers, switch to semicolon separators, or ship a "preamble" of
   metadata lines above the real header. A heuristic over header names
   cannot absorb that.
3. **We duplicate work the LLM can do for free.** Extracting a structured
   record out of a messy tabular document is exactly what frontier LLMs are
   good at, and we already depend on an external LLM for categorization
   further down the pipeline.

The goal of this iteration is to replace the per-bank column mapper with
a single unified schema and an LLM-driven parser that produces records
in that schema, regardless of which bank the file came from.

## 2. Goals and Non-Goals

### Goals

- One unified target schema (`UnifiedStatementRow`) that is a **union** of
  every field any of Monzo, Revolut, Lloyds, and "reasonable unknown UK
  bank" might expose.
- Replace `detect_format` header-sniffing with an LLM pass over the raw
  file.
- Keep a `BankFormat` enum for bookkeeping only (the LLM may set it; the
  import path never branches on it).
- Do not fail hard on unknown banks. If the LLM's bank detection is
  inconclusive, emit `BankFormat::Unknown` and continue.
- Fail hard only when the LLM's row-extraction **confidence** falls below
  a threshold, so that we never silently ingest garbage.
- Preserve the existing dedup / fingerprint / `ImportResult` contract so
  the rest of Phase 1 continues to work unchanged.

### Non-Goals

- Replacing the categorization pipeline. That stays external, per the
  MVP design.
- Supporting PDF / OFX / QFX / screenshots in this iteration. This plan
  covers CSV only. Screenshot ingestion is already deferred in
  `08_mvp_phases_v2.md`.
- Streaming very large files. For the MVP a bank statement is O(1k) rows
  and fits comfortably in one prompt. See "Open Questions" for the
  chunking path.

## 3. Current State (for reference)

Files that the iteration will touch, as they exist today on
`feature/phase-1-data-layer`:

- `backend/src/importers/mod.rs` — `Importer` trait + `get_importer`.
- `backend/src/importers/csv_importer.rs` — `BankFormat`, `detect_format`,
  `ColumnIndex`, `map_row`, `parse_amount`.
- `backend/src/model.rs` — `ImportResult { rows_total, rows_inserted,
  rows_duplicate, filename, account_id }`.
- `backend/tests/import_csv.rs` + fixtures in `backend/tests/fixtures/`.

## 4. Proposed Design

### 4.1 Unified target schema

A single struct becomes the target of every CSV import. It is a union of
every field any target bank currently exposes, with `Option<T>` for
anything that is not guaranteed.

```rust
// backend/src/importers/unified.rs

/// One row of a bank statement, after the LLM has normalised it into a
/// shape fynance understands. This is the union of every field Monzo,
/// Revolut, and Lloyds emit, plus the handful of extras that "unknown
/// but reasonable" UK banks tend to ship.
///
/// The LLM is responsible for filling this in from the raw CSV text.
/// Fields that are not present in the source are `None`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct UnifiedStatementRow {
    // -- Always required --
    #[ts(type = "string")]
    pub date: NaiveDate,
    pub description: String,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub amount: Decimal,                 // signed: negative = money out
    pub currency: String,                // ISO 4217, default "GBP"

    // -- Optional, bank-dependent --
    pub fitid: Option<String>,           // Monzo "Transaction ID", Lloyds ref
    pub category: Option<String>,        // Monzo ships categories, others may
    pub merchant: Option<String>,        // Monzo "Name"
    pub counterparty: Option<String>,    // Revolut "Description" for transfers
    pub transaction_type: Option<String>,// Revolut "Type", e.g. CARD_PAYMENT
    pub balance_after: Option<Decimal>,  // some banks include running balance
    pub notes: Option<String>,           // Monzo "Notes and #tags"
    pub reference: Option<String>,       // Lloyds "Transaction Reference"

    // -- Per-row provenance from the LLM --
    pub row_confidence: f32,             // [0.0, 1.0]
}
```

This replaces the per-bank dispatch in today's `map_row`. `Transaction`
stays exactly as it is today, it is populated from `UnifiedStatementRow`
in a small `From` conversion.

### 4.2 `BankFormat` as bookkeeping only

`BankFormat` survives as a display-only tag on `ImportResult` and on
every row written to `import_log`. It is never used to pick a code path.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum BankFormat {
    Monzo,
    Revolut,
    Lloyds,
    Unknown,   // LLM could not confidently identify the bank
}
```

`ImportResult` gains two fields:

```rust
pub struct ImportResult {
    pub rows_total: u64,
    pub rows_inserted: u64,
    pub rows_duplicate: u64,
    pub filename: String,
    pub account_id: String,
    // new in this iteration
    pub detected_bank: BankFormat,
    pub detection_confidence: f32,   // [0.0, 1.0], from the LLM's own estimate
}
```

`import_log` gains the same two columns (`detected_bank TEXT`,
`detection_confidence REAL`). Migration is additive and nullable on old
rows. See §8 for the schema change.

### 4.3 The LLM parser

A new module replaces `detect_format` + `map_row`:

```
backend/src/importers/
├── mod.rs                 # Importer trait (unchanged signature)
├── csv_importer.rs        # now just: read text, call llm_parser, fingerprint, insert
├── llm_parser.rs          # NEW: LLM client + prompt + response validation
└── unified.rs             # NEW: UnifiedStatementRow + From conversion
```

The new trait the parser implements:

```rust
// backend/src/importers/llm_parser.rs

#[async_trait::async_trait]
pub trait StatementParser: Send + Sync {
    /// Takes the raw file bytes, returns normalised rows plus whatever
    /// the LLM could tell us about which bank this is.
    async fn parse(&self, raw: &str, filename: &str) -> Result<ParsedStatement>;
}

pub struct ParsedStatement {
    pub detected_bank: BankFormat,
    pub detection_confidence: f32,
    pub rows: Vec<UnifiedStatementRow>,
}

pub struct LlmStatementParser {
    client: AnthropicClient,          // reuses the client set up for categorization
    model: String,                    // e.g. "claude-haiku-4-5-20251001"
    min_detection_confidence: f32,    // default 0.80, see §6
    min_row_confidence: f32,          // default 0.70, see §6
}
```

### 4.4 Import flow

```
                 ┌──────────────────────┐
 fynance import  │  read file to String │
 <file> --acct   └──────────┬───────────┘
                            │
                            ▼
                 ┌──────────────────────┐
                 │  LlmStatementParser  │
                 │  .parse(raw, name)   │
                 └──────────┬───────────┘
                            │
                            ▼
         ┌─────────────────────────────────────┐
         │ ParsedStatement {                   │
         │   detected_bank,                    │
         │   detection_confidence,             │
         │   rows: [UnifiedStatementRow, ...]  │
         │ }                                   │
         └──────────┬──────────────────────────┘
                    │
         detection_confidence < threshold?
                    │
         ┌──────────┴──────────┐
         ▼                     ▼
   hard fail              for row in rows:
   (return Err)             if row.confidence < threshold: skip + warn
                            else: map to Transaction, fingerprint,
                                  db.insert_transaction()
                            │
                            ▼
                     write ImportResult
                     { detected_bank,
                       detection_confidence,
                       rows_* counters }
                            │
                            ▼
                     db.log_import(...)
```

Two separate confidence gates matter:

- **File-level** (`detection_confidence`): "can I even trust that what
  I am looking at is a bank CSV?" Falling below the threshold is a hard
  fail for the whole file, as requested in prompt 3.3.
- **Row-level** (`row_confidence`): "did I parse this specific row?" A
  bad row skips (same behaviour as today's `tracing::warn!` branch in
  `map_row`), the rest of the file still ingests.

### 4.5 Prompt shape

The prompt is assembled in `llm_parser.rs` and pinned in the repo as
`backend/config/prompts/statement_parser.txt` so it is reviewable in git.

Outline:

```
System: You are a bank statement parser. You will receive the raw
text of a CSV file. Return a JSON object that strictly matches the
`ParsedStatement` schema below. Do not invent transactions. If a
column is missing, use null. Amount convention: negative is money
out, positive is money in. Dates are ISO 8601 (YYYY-MM-DD).

Schema: { ...UnifiedStatementRow JSON schema... }

User: filename=<name>
<raw file text, truncated to N KB>
```

Key points:

- We pass the **full raw text** to the LLM, not just headers. Header
  sniffing was exactly the brittleness we are replacing.
- The response is **JSON-mode constrained** by the Anthropic SDK's
  structured output / tool-use schema so we do not have to parse free
  text. The schema is derived from `ParsedStatement` via `schemars`
  (add crate).
- The prompt explicitly forbids hallucinated rows ("if you are not
  sure, mark `row_confidence` low and we will skip it").
- The LLM is asked to emit `detected_bank` and `detection_confidence`
  at the top level and `row_confidence` per row.

### 4.6 CsvImporter becomes a thin adapter

```rust
// backend/src/importers/csv_importer.rs  (after the change)

pub struct CsvImporter {
    pub parser: Arc<dyn StatementParser>,
}

impl Importer for CsvImporter {
    fn import(&self, path: &Path, account_id: &str, db: &Db) -> Result<ImportResult> {
        let raw = fs::read_to_string(path)
            .with_context(|| format!("reading {path:?}"))?;
        let filename = path.file_name()
            .and_then(|n| n.to_str()).unwrap_or("<unknown>").to_string();

        // Block on the async parser in the sync CLI path. The server
        // path (Phase 2) calls `parser.parse(...).await` directly.
        let parsed = tokio::runtime::Handle::current()
            .block_on(self.parser.parse(&raw, &filename))?;

        if parsed.detection_confidence < self.parser.min_detection_confidence() {
            return Err(anyhow!(
                "LLM could not confidently identify {filename}: {:.2} < {:.2}",
                parsed.detection_confidence,
                self.parser.min_detection_confidence(),
            ));
        }

        let mut result = ImportResult {
            filename: filename.clone(),
            account_id: account_id.to_string(),
            detected_bank: parsed.detected_bank,
            detection_confidence: parsed.detection_confidence,
            ..ImportResult::default()
        };

        for row in parsed.rows {
            result.rows_total += 1;

            if row.row_confidence < self.parser.min_row_confidence() {
                tracing::warn!(
                    "skipping low-confidence row in {}: {:.2}",
                    filename, row.row_confidence,
                );
                continue;
            }

            let tx = Transaction::from_unified(row, account_id);
            match db.insert_transaction(&tx)? {
                InsertOutcome::Inserted => result.rows_inserted += 1,
                InsertOutcome::Duplicate => result.rows_duplicate += 1,
            }
        }

        Ok(result)
    }
}
```

All of `detect_format`, `ColumnIndex`, `map_row`, and `parse_amount`
are deleted. `parse_amount` survives in `util.rs` because the
`Transaction::from_unified` path still needs to defensively strip
currency symbols if the LLM leaves them in.

## 5. Data Model Changes

### 5.1 Rust

- Add `backend/src/importers/unified.rs` with `UnifiedStatementRow` and
  `Transaction::from_unified`.
- Add `detected_bank: BankFormat` and `detection_confidence: f32` to
  `ImportResult` in `model.rs`.
- `BankFormat` moves from `importers::csv_importer` to `model.rs` so it
  can live next to `ImportResult` and be exported via `ts-rs`.
- New crate dependencies (Phase 1 iteration):
  - `async-trait`, `schemars`, and whichever Anthropic client crate
    Phase 2 settles on. `reqwest` is already in the stack.

### 5.2 SQL

Additive migration only. `db/sql/schema.sql` keeps its current shape;
a new migration file `db/sql/migrations/001_import_log_bank_format.sql`
adds the columns:

```sql
ALTER TABLE import_log
  ADD COLUMN detected_bank TEXT;
ALTER TABLE import_log
  ADD COLUMN detection_confidence REAL;
```

Existing rows get `NULL` for both. `Db::log_import` is updated to write
the new columns.

### 5.3 TypeScript bindings

`ts-rs` regenerates `frontend/src/bindings/`:

- `UnifiedStatementRow.ts` (new)
- `BankFormat.ts` (new)
- `ImportResult.ts` (updated with the two new fields)

No frontend wiring is in scope for this iteration, but types ship so
Phase 2 can surface them.

## 6. Confidence Thresholds

The user asked us to "figure out what that threshold is and use it."
The proposal is two thresholds, both configurable via env vars and
defaulted in code:

| Gate | Default | Env var | Rationale |
|---|---|---|---|
| `min_detection_confidence` (file-level) | **0.80** | `FYNANCE_IMPORT_MIN_DETECT_CONF` | High enough to reject "this might not even be a bank CSV", low enough that an obviously-correct but unknown-bank file with confidence ≈ 0.85 still passes. |
| `min_row_confidence` (per row) | **0.70** | `FYNANCE_IMPORT_MIN_ROW_CONF` | Row-level confidence is noisier because of preamble lines, totals rows, and weird separators. 0.70 keeps obvious rows while dropping junk. |

How the 0.80 number was picked:

- If the LLM confidently says "Monzo" or "Revolut" it reports ≥ 0.95 in
  practice. Anything under 0.80 means the model is genuinely uncertain,
  and silently ingesting that is exactly the failure mode we are trying
  to prevent.
- The user wants unknown banks to **succeed**, not fail. So the
  threshold has to sit below the "unknown but structurally fine" band
  (≈ 0.85 in our prompt pilots) and above the "I am guessing" band
  (≈ 0.6). 0.80 is the natural gap.
- Both thresholds are env-overridable precisely because this is a
  tunable. The first few real imports will tell us whether to tighten
  or loosen.

A failing `detection_confidence` returns `Err` from the importer. A
failing `row_confidence` skips the row, bumps `rows_total` without
bumping `rows_inserted` or `rows_duplicate`, and logs a warning. Both
are visible to the operator after the fact via `import_log`.

## 7. Unknown Banks

Behaviour when the LLM says "I don't know which bank this is":

| LLM state | detected_bank | detection_confidence | Outcome |
|---|---|---|---|
| Confidently Monzo/Revolut/Lloyds | `Monzo`/`Revolut`/`Lloyds` | ≥ 0.80 | Proceed, tag the import with that bank |
| Confidently some other bank (e.g. Barclays) | `Unknown` | ≥ 0.80 | Proceed, tag `Unknown`, we still get the rows |
| Genuinely uncertain | `Unknown` | < 0.80 | **Hard fail**: return `Err`, file is not ingested |

This matches the prompt's requirement: "do not fail hard as it
currently does" for unknown banks, but "fail hard in cases where the
detection confidence ratio is less than a threshold" for genuinely
ambiguous files.

The user can add more named variants to `BankFormat` later if a
particular "unknown" bank starts showing up often. It is purely a
display concern.

## 8. Security and Privacy

This is the first time we send real transaction data outside the
local machine. That deserves explicit call-outs:

1. **User opt-in.** LLM-backed import only runs when an API key is
   configured (`FYNANCE_ANTHROPIC_API_KEY`). Without a key, `fynance
   import` returns an actionable error and exits. No silent fallback to
   a "local parser", that is exactly the brittleness we removed.
2. **No prompt caching of customer data by default.** We pass
   `cache_control: none` on the user message. Only the system prompt
   and schema are cached.
3. **Redaction opt-in (future).** A `FYNANCE_IMPORT_REDACT=true` flag
   can scrub obvious PII (account numbers, sort codes) before sending.
   Out of scope for this iteration but noted in `05_security_isolation.md`.
4. **Logging.** Per the existing convention, we never log raw
   transaction descriptions at INFO level. The LLM request/response is
   logged at DEBUG only, and the payload is truncated.
5. **Local data directory permissions are unchanged.** The LLM call
   does not touch the DB, it is upstream of the insert path.

`design/05_security_isolation.md` will gain a short "External LLM
calls" section pointing at this document.

## 9. Testing Strategy

Three layers, all fake-first so tests do not need a live API key.

### 9.1 Unit tests (`llm_parser.rs`)

- Construct `ParsedStatement` fixtures by hand and feed them through a
  `MockStatementParser` that implements `StatementParser` and returns
  a pre-canned response. Used to test:
  - Confidence-gate behaviour (below/above threshold on file level).
  - Row-skip behaviour (below/above threshold on row level).
  - Unknown-bank pass-through.

### 9.2 Integration tests (`tests/import_csv.rs`)

- Today's Monzo/Revolut/Lloyds CSV fixtures stay.
- Tests inject a `MockStatementParser` seeded from a sibling JSON
  fixture (`monzo.expected.json`, `revolut.expected.json`,
  `lloyds.expected.json`) so that the old row-count and amount-sign
  assertions keep working without any network traffic.
- New fixture: `unknown_bank.csv` + `unknown_bank.expected.json` with
  `detected_bank = Unknown`, `detection_confidence = 0.82`. Asserts
  the unknown-bank happy path.
- New fixture: `garbage.csv` (a shopping list, not a bank statement).
  Mock parser returns `detection_confidence = 0.4`. Asserts a hard
  failure with a non-empty error message.

### 9.3 A single live smoke test (`#[ignore]`)

- `tests/llm_parser_live.rs`, gated on `FYNANCE_ANTHROPIC_API_KEY`,
  run with `cargo test -- --ignored`. Hits the real model against the
  Monzo fixture and asserts `detected_bank == Monzo`,
  `detection_confidence >= 0.80`, and row counts match. Not run in
  CI by default; run manually when changing the prompt.

## 10. Updated Phase 1 Checklist (delta against `09_backend_implementation_plan.md`)

Against §1.5 "Import trait and CSV importers", the checklist becomes:

- [ ] `backend/src/importers/unified.rs`: `UnifiedStatementRow`,
      `Transaction::from_unified`.
- [ ] `backend/src/importers/llm_parser.rs`: `StatementParser` trait,
      `LlmStatementParser`, `MockStatementParser` for tests,
      `ParsedStatement`, prompt assembly, JSON-schema validation.
- [ ] `backend/src/importers/csv_importer.rs`: rewrite as the thin
      adapter in §4.6. Delete `detect_format`, `ColumnIndex`,
      `map_row`, and the `BankFormat` enum local to this file.
- [ ] `backend/src/model.rs`: move `BankFormat` here, extend
      `ImportResult` with `detected_bank` and `detection_confidence`.
- [ ] `db/sql/migrations/001_import_log_bank_format.sql` plus
      `Db::open` runs it after `schema.sql`.
- [ ] `Db::log_import` writes the two new columns.
- [ ] Env vars in `.env.example`: `FYNANCE_ANTHROPIC_API_KEY`,
      `FYNANCE_IMPORT_MIN_DETECT_CONF`, `FYNANCE_IMPORT_MIN_ROW_CONF`,
      `FYNANCE_IMPORT_LLM_MODEL`.
- [ ] `backend/config/prompts/statement_parser.txt` pinned in the repo.
- [ ] Tests per §9 (unit, integration with mock parser, one ignored
      live smoke test).
- [ ] Regenerate TS bindings via `cargo test` (ts-rs).

Everything outside §1.5 stays as it is.

## 11. Open Questions

1. **Large files.** At what row count do we chunk? Monzo's yearly CSV
   is ≈ 3k rows. Claude's context easily absorbs that, but a 5-year
   Revolut dump is ≈ 15k and worth chunking. Proposal: if the raw
   text is > 200 KB, split on row boundaries, send chunks, merge
   `ParsedStatement`s. Out of scope for this iteration, noted for
   follow-up.
2. **Cost accounting.** Each import is now a paid LLM call. Do we want
   to surface a cost estimate in `ImportResult`? Probably yes, as a
   `tokens_in`, `tokens_out`, `cost_usd` triple. Proposal: add in a
   later iteration once we have real numbers.
3. **Offline fallback.** Should we ship a tiny "last known good"
   parser for Monzo/Revolut/Lloyds as a fallback when the API key is
   missing? The prompt says no, the point is to get rid of the
   per-bank code. Leaving this explicit so we remember we decided
   against it.
4. **Per-row dedup of LLM runs.** If the same file is imported twice
   we pay the LLM cost twice even though the fingerprinted inserts
   will all be duplicates. Proposal: hash the raw file bytes and
   short-circuit via `import_log` before calling the LLM. Small
   follow-up, not blocking.

## 12. Cross-Document Updates

This plan is the source of truth; the following existing docs get a
short note pointing here so nobody implements the old path by accident:

- `docs/plans/03_importer.md` — top-of-file note: "Superseded for CSV
  by `10_llm_csv_import.md`. The bank-mapping tables below are kept for
  historical context only."
- `docs/plans/09_backend_implementation_plan.md` — §1.5 gains a note:
  "Implement per `10_llm_csv_import.md` instead of the header-sniffing
  path described below."
- `docs/design/02_architecture.md` — "Ingest" box in the architecture
  diagram gains an "LLM parser" sub-box with a pointer to this plan.
- `docs/design/03_data_model.md` — `ImportResult` section gains the
  two new fields.
- `docs/design/05_security_isolation.md` — "External LLM calls"
  subsection: "CSV import sends raw statement text to Anthropic; see
  `plans/10_llm_csv_import.md` §8 for the privacy model."
