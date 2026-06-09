# Multi-Institution Data Import Workflow with Dryrun Preview - Gaps & Implementation Plan

---

## Problem Statement

### The Core Problem

Users of Fynance need to import financial data (transactions and investment holdings) from multiple financial institutions (banks, brokerages, trading platforms) to maintain accurate records of their finances and investments. However, the current system has significant gaps that make this workflow fragmented and risky:

#### Current Pain Points

1. **Incomplete Import Capability**
   - Transactions can be imported via CSV upload with automatic format detection (Monzo, Revolut, Lloyds, etc.), but with **no preview before commitment**
   - Holdings can be imported only via direct API calls with manually formatted JSON—**no UI exists**
   - No integrated workflow to import both data types from a single institution export
   - Users cannot see what will be imported before the data is permanently committed to the database

2. **Lack of Safety / Preview Mechanism**
   - Transaction imports commit immediately to the database with **no rollback** if the user made a mistake
   - No visibility into duplicate detection (does the system think this transaction is new or already imported?)
   - No way to preview how holdings will be merged (new positions vs. updating existing ones)
   - High risk of accidental data corruption or duplicate imports if a statement is uploaded twice

3. **Institutions Treated Inconsistently**
   - **Transactions:** Supported from Monzo, Revolut, Lloyds via CSV with LLM-assisted format detection; format auto-detected
   - **Holdings:** No extraction logic exists; must be manually formatted as JSON; no format detection
   - Different institutions export data in different formats (CSV, Excel, PDF, JSON), but system only handles CSV
   - Adding support for a new institution is manual and error-prone
   - No standardized way to specify which fields map to which data (symbol, quantity, value, date, etc.)

4. **User Experience Friction**
   - Multi-step, error-prone manual process: export → format → validate → upload → hope it works
   - No clear feedback on what the system detected or how it will be processed
   - If import fails, user has no preview of what went wrong
   - Holdings import requires external tools to prepare JSON; not integrated into UI

5. **Data Integrity Risk**
   - CSV imports bypass preview; duplicates can only be detected after commit (via fingerprint)
   - Holdings can be partially imported if validation fails mid-transaction
   - No atomic transactions across multiple data types (transactions + holdings from same export)
   - Difficult to recover from a bad import without manual database editing

### What We Need to Achieve

A **unified, institution-agnostic import workflow** where:

- Users can **export a file from ANY financial institution** (bank, brokerage, trading platform)
- The system **automatically detects the institution and format** (CSV, Excel, PDF, JSON, etc.)
- The system **automatically extracts relevant data** (both transactions and holdings) from that export
- A **comprehensive preview is shown** before ANY data is written to the database
  - What transactions will be added (new) vs. skipped (duplicates)?
  - What holdings will be added (new) vs. updated (modify)?
  - Any conflicts, errors, or data validation issues?
- The user can **review the preview and confirm** in one action
- The entire import is **atomic** (all succeeds or all fails; no partial state)
- The process is **repeatable and safe** (same file imported twice should be idempotent)

### Why This Matters

- **Safety:** Preview before commit prevents accidental data corruption
- **Transparency:** Users understand exactly what their data will look like
- **Flexibility:** Any financial institution can be supported (not just hardcoded ones)
- **Trust:** Dryrun + preview builds confidence that imports are correct
- **Efficiency:** Single upload replaces multi-step manual process
- **Scalability:** New institutions can be added without code changes (format detection + extraction)

---

## Scope: Institution Agnostic

All requirements should be designed to work with **ANY financial institution**, including but not limited to:

### Banks & Checking Accounts
- Revolut, Monzo, Lloyds, HSBC, Barclays, NatWest, Wise, etc.
- Export formats: CSV, OFX, MT940, PDF statements

### Brokerages & Investment Platforms
- Trading 212, eToro, Freetrade, AJ Bell, Interactive Brokers, etc.
- Export formats: CSV (positions, transactions, holdings), PDF statements, Excel sheets
- Data types: Equity holdings, ETFs, bonds, crypto, options, cash balances

### Pension Providers
- Vanguard, Fidelity, Scottish Widows, etc.
- Formats: PDF statements, Excel downloads, CSV summaries
- Data: Holdings snapshots, income distributions

### Specialist Platforms
- Crypto exchanges (Kraken, Coinbase, Gemini, etc.): JSON, CSV
- FX platforms (Wise, OFX, etc.): CSV, custom formats
- Savings accounts (Chip, Plum, etc.): CSV, API

### Current Hardcoded Institutions (Revolut, Monzo, Lloyds)
These should continue to work with the new system, but should be treated as plugins, not special cases.

---

## Current State Summary

| Feature | Transactions | Holdings |
|---------|--------------|----------|
| **File Upload UI** | ✅ Exists (CSV only, via `FileUpload` component) | ❌ Missing (no UI; JSON API only) |
| **API Endpoint** | ✅ `POST /api/import/csv` (multipart) + `POST /api/import` (JSON) + `POST /api/import/bulk` (multi-file) | ✅ `POST /api/holdings/import` (JSON body only, no file upload) |
| **Format Detection** | ✅ LLM-based (Claude Haiku returns `detected_bank` + confidence; identifies Monzo, Revolut, Lloyds) | ❌ None (user must pre-format as JSON matching `Holding` struct) |
| **Automatic Extraction** | ✅ LLM Parser sends raw CSV to Claude Haiku, extracts structured transactions with confidence scores | ❌ None |
| **Dryrun / Preview** | ❌ Missing (all three endpoints commit immediately; no dry_run param exists) | ✅ Exists (`?dry_run=true` on `POST /api/holdings/import`; returns `HoldingPreview[]` with new/modify status) |
| **Frontend Preview UI** | ⚠️ `PreviewTable` exists but shows POST-commit aggregate stats (inserted/duplicate counts), NOT a pre-commit per-row preview | ❌ Missing (no component renders `HoldingPreview` data) |
| **Multi-Institution Support** | ⚠️ LLM handles any CSV format (not hardcoded parsers), but confidence threshold gates acceptance | ❌ None (no file parsing at all) |
| **Atomic Multi-Data Import** | ❌ Missing (transactions only; each row inserted individually via `INSERT OR IGNORE`) | N/A (separate endpoint) |

### Key Technical Details

- **Transaction deduplication:** SHA-256 fingerprint of `(date_ISO, amount_string, account_id)` computed in `importers/unified.rs:82`. The `transactions` table has a UNIQUE constraint on `fingerprint`. `INSERT OR IGNORE` silently skips duplicates. There is no way to preview which rows would be duplicates without attempting the insert.
- **Holdings deduplication:** Composite key `(account_id, symbol, sub_account, as_of)`. `upsert_holdings()` updates existing rows. `dry_run_holdings()` checks existence via SELECT before preview.
- **LLM parsing flow:** Raw CSV (truncated to 200KB) is sent to Claude Haiku via Anthropic API with a tool_use schema. The response includes per-row confidence scores. Rows below `min_row_confidence` (default 0.70) are silently dropped. The file-level `detection_confidence` (default threshold 0.80) gates the entire file.
- **Frontend import flow:** Import page has steps: account-select -> upload -> result -> complete. The `handleSubmit()` function (import.tsx:62) calls `api.importCsv()` which immediately commits. The "result" step shows aggregate counts, not a preview.

---

## Success Metrics

When complete, users should be able to:

1. ✅ Export data from **any** financial institution in **any** format
2. ✅ Upload to Fynance frontend with **zero manual formatting**
3. ✅ See a **comprehensive preview** of both transactions and holdings
4. ✅ Understand **exactly what will change** (new, modify, skip, error)
5. ✅ Click **one "Confirm" button** to commit everything
6. ✅ Know that **the import is safe and repeatable** (can re-import same file)
7. ✅ Receive **clear error messages** if something goes wrong
8. ✅ Have **no data loss or corruption** even if they mess up

---

## Implementation Gaps

### **A. Backend: Dryrun for Transactions**

#### Gap A1: Transaction Import Dryrun Query Parameter
- **Status:** ❌ Not implemented
- **What:** `POST /api/import/csv?dry_run=true` and `POST /api/import?dry_run=true` don't exist
- **Where:** `backend/src/server/routes/import_api.rs`
  - `import_json()` at lines 34-77: accepts `Json<ImportPayload>` only, no query params at all
  - `import_csv()` at lines 86-146: uses `CsvImportQuery` (line 82) which only has `account: Option<String>`
  - `import_bulk()` at lines 150-307: no query params (multipart only)
- **What's Needed:**
  - Add `dry_run: Option<bool>` to `CsvImportQuery` struct (line 82, currently has only `account`)
  - Create a new query struct for `import_json` (currently takes no query params, only JSON body)
  - For CSV path: when dry_run=true, still run LLM parsing + `Transaction::from_unified()` to build transactions, but instead of calling `db.insert_transaction()`, call a new `db.dry_run_transactions()` (see A3)
  - For JSON path: when dry_run=true, still validate account/currencies but call `db.dry_run_transactions()` instead of `db.insert_transactions_bulk()`
  - Skip `db.log_import()` when dry_run=true
  - Note: `import_bulk` is lower priority for dry_run since it's primarily used by the wizard; consider deferring
- **Effort:** Low (2-3 hours)
- **Dependencies:** A3 (needs the DB method to exist)
- **Blocks:** Frontend integration

#### Gap A2: Transaction Import Dryrun Response Type
- **Status:** ❌ Not defined
- **What:** Need a response struct for dryrun preview of transactions
- **Where:** `backend/src/model.rs`
- **Current context:** The existing `ImportResult` struct has `rows_total`, `rows_inserted`, `rows_duplicate`, `filename`, `account_id`, `detected_bank`, `detection_confidence`, `errors`. It reports aggregate counts but provides NO per-row detail (e.g., which specific rows are new vs. duplicate).
- **What's Needed:**
  - A new response type that includes per-row preview information:
    - Each row needs: index, date, description, amount, currency, status (new/duplicate)
    - Duplicate rows should show the matching existing transaction (date, description, amount) so users can visually confirm it's truly a duplicate
  - The response should also include the original parsed data (or a commit token) so the frontend can confirm the import without re-uploading/re-parsing
  - Note: The `ImportPayload` struct already exists and could serve as the commit payload for the JSON path. For the CSV path, the system re-uploads and re-parses on confirm (since the LLM output is not persisted), so the response should include the parsed `ImportPayload` directly.
  - Design decision needed: Should the dryrun response carry the full parsed payload (so confirm just POSTs to `/api/import`), or should the server cache the parse result (complexity)?
- **Effort:** Low (1-2 hours)
- **Dependencies:** None
- **Blocks:** A1, A3, frontend integration

#### Gap A3: Transaction Import Dryrun Handler Logic
- **Status:** ❌ Not implemented
- **What:** Backend logic to preview transaction inserts without committing
- **Where:** `backend/src/storage/db.rs` and `backend/src/server/routes/import_api.rs`
- **Current dedup mechanism:** `insert_transaction()` (db.rs:685) uses `INSERT OR IGNORE` on the `fingerprint` column's UNIQUE constraint. The fingerprint is SHA-256 of `(date_iso, amount_str, account_id)` computed in `unified.rs:82` via `crate::util::fingerprint()`. A duplicate is detected only by the DB returning 0 affected rows. There is NO standalone "check if exists" query.
- **What's Needed:**
  - Create `fn dry_run_transactions(&self, account_id: &str, transactions: &[ImportTransaction]) -> Result<Vec<TransactionPreviewRow>>` in `Db`
  - For each transaction:
    1. Compute fingerprint using same logic as `Transaction::from_unified()` (sha256 of date+amount+account_id)
    2. Query: `SELECT id, date, description, amount FROM transactions WHERE fingerprint = ?1`
    3. If found: mark as "duplicate" and attach existing transaction details
    4. If not found: mark as "new"
  - This mirrors the pattern of `dry_run_holdings()` (db.rs:2680) which queries existing holdings by composite key and returns `HoldingPreview` with status
  - Important: The fingerprint computation currently lives in `Transaction::from_unified()` (importers/unified.rs:82). For dry_run, we need the fingerprint without building a full `Transaction`. Either extract the fingerprint logic or call `from_unified()` and just use the fingerprint field.
  - For the JSON path (`insert_transactions_bulk`, db.rs:721), the fingerprint is computed inside a loop at ~line 770. The same extraction approach applies.
- **Effort:** Medium (3-4 hours)
- **Dependencies:** A2
- **Blocks:** Frontend integration

---

### **B. Backend: Holdings Format Detection & Extraction**

#### Gap B1: Holdings Statement Format Parsers (Institution-Agnostic)
- **Status:** ❌ Not implemented
- **What:** Automatic extraction of holdings from any financial institution's statement export
- **Where:** New parsers in `backend/src/importers/` directory
- **Current state:** The `importers/` directory has 4 files: `mod.rs`, `csv_importer.rs`, `llm_parser.rs`, `unified.rs`. All are exclusively for transaction parsing. The existing `Importer` trait (mod.rs:19) returns `ImportResult` which is transaction-specific. There is NO holdings-related parsing infrastructure whatsoever. Holdings are imported ONLY via pre-formatted JSON to `POST /api/holdings/import`.
- **Architecture note:** The existing `LlmStatementParser` sends raw CSV to Claude Haiku with a tool_use schema that outputs `ParsedStatement { detected_bank, detection_confidence, rows: Vec<UnifiedStatementRow> }`. This is exclusively transaction-oriented (date, description, amount, currency fields). A holdings parser would need a completely different output schema (symbol, quantity, price_per_unit, value, holding_type).
- **What's Needed:**
  - Create a new trait separate from the transaction `Importer` trait:
    ```rust
    pub trait HoldingsParser: Send + Sync {
      fn name(&self) -> &str;
      fn detect(&self, file_content: &str, filename: &str) -> f32;
      fn parse(&self, file_content: &str, account_id: &str) -> Result<Vec<Holding>>;
    }
    ```
  - Key difference from transactions: Holdings parsers DON'T need an LLM. Institution exports for holdings tend to be structured (CSV with headers like "Instrument", "Quantity", "Value"). Header detection + column mapping is sufficient for most institutions.
  - Implement parsers for:
    - **Trading 212:** CSV with columns like `Ticker`, `Shares`, `Price per share`, `Value (GBP)`, `Currency (Price / share)`
    - **Revolut:** Statement PDF's holdings section (or their CSV export if available)
    - **Generic CSV:** User provides a column mapping (symbol=col1, quantity=col2, etc.)
  - Each parser must produce `Vec<Holding>` matching the existing `Holding` struct in model.rs (fields: account_id, symbol, name, holding_type, quantity, price_per_unit, value, currency, as_of, short_name, sub_account, is_closed)
  - Start with CSV only. PDF/Excel support can come later as those require additional crate dependencies (pdf-extract, calamine).
- **Effort:** High (10-14 hours for framework + 2-3 initial parsers)
- **Dependencies:** None
- **Blocks:** B2, B3

#### Gap B2: Holdings Format Auto-Detection (Multi-Parser)
- **Status:** ❌ Not implemented
- **What:** Automatically detect which institution/format the uploaded holdings file is from
- **Where:** New module in `backend/src/importers/` (e.g., `holdings_detector.rs`)
- **Current state:** For transactions, the LLM handles detection implicitly (returns `detected_bank` and `detection_confidence` in its response). For holdings, there is NO detection mechanism. The existing `get_importer()` in mod.rs only checks file extension (line 32: `match ext.as_deref()`).
- **What's Needed:**
  - A registry that iterates registered `HoldingsParser` implementations
  - Calls each parser's `detect()` method against the file content
  - Returns the best match above a confidence threshold
  - Detection is header-based for CSVs: e.g., if headers contain "Ticker" + "Shares" + "Result (GBP)" that's Trading 212 with high confidence
  - Edge case: no parser is confident. Return an error suggesting the user use the JSON API directly or request support for their institution.
  - No runtime plugin registration needed for V0. Compile-time registration is fine.
- **Effort:** Low-Medium (2-3 hours, since detection is straightforward header matching)
- **Dependencies:** B1 (needs at least one parser to register)
- **Blocks:** B3

#### Gap B3: API Endpoint for Holdings File Upload + Parse + Preview
- **Status:** ❌ Not implemented
- **What:** Allow frontend to upload a holdings file (CSV) and get parsed + previewed holdings
- **Where:** New handler in `backend/src/server/routes/holdings.rs`
- **Current state:** The existing `POST /api/holdings/import` endpoint (holdings.rs:373) accepts ONLY `Json<HoldingsImportPayload>` (pre-formatted JSON). There is NO multipart file upload endpoint for holdings. The dry_run on this endpoint (line 394) previews pre-formatted JSON, not raw files.
- **What's Needed:**
  - New endpoint: `POST /api/holdings/import/file?account_id=...&dry_run=true` (multipart file upload)
  - Flow: receive file -> detect institution (B2) -> parse to `Vec<Holding>` (B1) -> if dry_run, call `db.dry_run_holdings()` -> return preview
  - Response should include: detected institution, confidence, the parsed holdings, and their dry_run preview (new/modify status)
  - If not dry_run, proceed directly to `db.upsert_holdings()`
  - This endpoint is the "file upload equivalent" of the existing JSON endpoint, adding the parse step in between
- **Effort:** Medium (3-4 hours)
- **Dependencies:** B1, B2
- **Blocks:** Frontend holdings file upload

#### Gap B4: Combined Transaction + Holdings Import Endpoint (Any Institution)
- **Status:** ❌ Not implemented
- **What:** Single file upload that extracts both transactions AND/OR holdings from any institution's export
- **Where:** `backend/src/server/routes/import_api.rs`
- **Current state:** Transactions and holdings are completely separate code paths today. Transactions go through `import_csv()` which uses the LLM parser. Holdings go through `import_holdings()` which expects pre-formatted JSON. There is no shared pipeline or unified entry point.
- **Design consideration:** Most institution exports contain EITHER transactions OR holdings, not both. A Revolut CSV statement is transactions. A Trading 212 positions export is holdings. The "unified" endpoint's primary value is detecting WHICH type the file contains and routing appropriately, rather than handling files that genuinely contain both.
- **What's Needed:**
  - New endpoint: `POST /api/import/unified?account_id=...&dry_run=true` (multipart file upload)
  - Detection logic: try transaction parsers AND holdings parsers, see which match
  - If transaction-like: route to LLM parser path (existing `import_csv` flow)
  - If holdings-like: route to holdings parser path (B1)
  - Return unified response indicating what was found and previewing it
  - Truly combined files (both data types) can be deferred to V1
- **Effort:** Medium (5-7 hours)
- **Dependencies:** A1/A3 (transaction dryrun), B3 (holdings file upload)
- **Blocks:** Frontend unified import flow

---

### **C. Frontend: Dryrun Preview UI**

#### Gap C1: Transaction Dryrun Preview Integration
- **Status:** ⚠️ Partial (PreviewTable exists but shows post-commit results, not a pre-commit preview)
- **What:** Update the import flow to show a dryrun preview BEFORE committing, with a "Confirm" button to actually write data
- **Where:** `frontend/src/pages/import/preview_table.tsx` + `import.tsx`
- **Current state:** The existing `PreviewTable` component (preview_table.tsx) receives an `ImportResult` and displays aggregate stats (total/inserted/duplicate counts) AFTER the import has already committed. The `import.tsx` page calls `api.importCsv()` on submit (line 68), which immediately writes to DB, then shows the result in the "result" step. There is no intermediate preview step. The user flow is: Upload -> Submit -> See committed result -> Done.
- **What's Needed:**
  - Change the import flow from "Upload -> Commit -> Show result" to "Upload -> Dryrun -> Preview -> Confirm -> Commit -> Show result"
  - `handleSubmit()` in import.tsx should first call dry_run=true, show preview
  - `PreviewTable` needs a complete redesign: instead of showing aggregate ImportResult counts, it needs to render PER-ROW data with status badges (NEW/DUPLICATE)
  - Add a "Confirm Import" button that calls the same endpoint WITHOUT dry_run
  - Add a "Cancel" button that discards the preview and goes back to upload
  - For the CSV path: the confirm step needs to re-upload the file (or the backend caches the parsed result, TBD in A2)
- **Effort:** Medium (4-5 hours)
- **Dependencies:** A1, A2, A3 (backend dryrun must exist first)
- **Blocks:** Full dryrun workflow

#### Gap C2: API Method for Transaction Dryrun
- **Status:** ❌ Not implemented
- **What:** Frontend API service method for transaction dryrun
- **Where:** `frontend/src/api/real_service.ts` (line 298: `importCsv`) + `frontend/src/api/service.ts` (line 125: interface)
- **Current state:** The `ApiService` interface (service.ts:125) has only `importCsv(accountId: string, file: File): Promise<ImportResult>`. The `RealApiService` implementation (real_service.ts:298) does `postMultipart` to `/api/import/csv?account=...`. There is NO dryrun variant and NO holdings import method in either file.
- **What's Needed:**
  - Add to `ApiService` interface:
    - `dryRunImportCsv(accountId: string, file: File): Promise<TransactionImportPreview>`
    - `confirmImportCsv(accountId: string, file: File): Promise<ImportResult>` (or reuse existing `importCsv`)
  - Add to `RealApiService`:
    - Same endpoint with `&dry_run=true` appended to query string
  - Import the `TransactionImportPreview` type from `@/bindings/` (generated from A2)
- **Effort:** Low (1-2 hours)
- **Dependencies:** A1 (backend endpoint), D1 (TypeScript binding)
- **Blocks:** C1

#### Gap C3: Holdings Upload & Format Detection UI Component
- **Status:** ❌ Not implemented
- **What:** UI component for uploading holdings files (CSV from brokerages) with detection + preview
- **Where:** `frontend/src/pages/import/` (new component)
- **Current state:** The import page (`import.tsx`) is exclusively for transaction CSV uploads. There is NO holdings import anywhere in the frontend UI. The only way to import holdings today is via direct API call (`POST /api/holdings/import` with JSON body). The frontend DOES display holdings in the portfolio page, but has no way to upload them.
- **What's Needed:**
  - A file drop zone that accepts CSV files from brokerage exports
  - After upload: show detected institution + parsed holdings in a preview table
  - Table columns: symbol, name, quantity, value, currency, status (NEW/MODIFY)
  - "Confirm Import" button to commit
  - Could be a separate tab/section on the import page, or a dedicated route
  - Simpler than C6 (unified): this is JUST holdings upload, not combined with transactions
- **Effort:** Medium (5-7 hours)
- **Dependencies:** B3 (backend file upload endpoint)
- **Blocks:** C4

#### Gap C4: Holdings Dryrun Preview Component
- **Status:** ⚠️ Partially covered by C3
- **What:** Show dryrun preview of holdings import (new vs. modify indicators)
- **Where:** Part of C3 component or a sub-component
- **Current state:** The `HoldingPreview` TypeScript binding already exists (`frontend/src/bindings/HoldingPreview.ts`) with fields: `account_id`, `symbol`, `sub_account`, `value`, `currency`, `as_of`, `status`, `existing_value`. The backend `dry_run_holdings()` returns this type. The ONLY missing piece is a frontend component that renders these.
- **What's Needed:**
  - A table component that renders `HoldingPreview[]`
  - Status column with "new" (green) and "modify" (amber) badges
  - For "modify" rows: show existing_value alongside new value
  - "Confirm Import" button
  - Can be a generic component reused by both C3 (file upload) and any future manual holdings entry
- **Effort:** Low-Medium (2-3 hours, since it's just a table rendering existing data)
- **Dependencies:** HoldingPreview type already exists in bindings
- **Blocks:** C3, C6

#### Gap C5: API Method for Holdings Import/Dryrun
- **Status:** ❌ Not implemented
- **What:** Frontend API service methods for holdings file upload and dryrun
- **Where:** `frontend/src/api/real_service.ts` + `service.ts`
- **Current state:** There are NO holdings import methods in the `ApiService` interface or `RealApiService`. The frontend has `getHoldings()` and `getHoldingsBatch()` for reading, but no write methods.
- **What's Needed:**
  - `uploadHoldingsFile(accountId: string, file: File, dryRun: boolean): Promise<HoldingsFileImportResponse>` (for file-based upload via B3)
  - `importHoldingsJson(payload: HoldingsImportPayload, dryRun: boolean): Promise<HoldingsDryRunResponse | HoldingsImportResult>` (for the existing JSON endpoint)
  - Types needed from bindings: `HoldingPreview` (already exists), a new `HoldingsFileImportResponse` (from B3)
- **Effort:** Low (1-2 hours)
- **Dependencies:** B3 (backend file endpoint)
- **Blocks:** C3, C4

#### Gap C6: Unified Import Workflow Interface (Any Institution)
- **Status:** ❌ Not implemented
- **What:** Single import interface that handles both transactions and holdings from any financial institution
- **Where:** `frontend/src/pages/import/import.tsx` (refactor + extend existing ImportPage)
- **Current state:** The existing import page (import.tsx) has a wizard-like flow with steps: "account-select" -> "upload" -> "result" -> "complete". It supports two modes: "single" (one account) and "wizard" (iterate through multiple accounts). Components: `FileUpload` (drop zone), `PreviewTable` (post-commit results), `ImportSummary` (final summary), `WizardProgress` (sidebar). All of this is transaction-only.
- **What's Needed:**
  - Add a detection step between upload and preview: Upload -> Detect -> Preview -> Confirm -> Done
  - After file upload, call the unified endpoint (B4) which detects whether it's transactions or holdings
  - Show appropriate preview: transaction preview (per-row new/duplicate) or holdings preview (new/modify)
  - Single "Confirm" button regardless of data type
  - Account selection must happen BEFORE upload (already exists in current flow)
  - Keep backward compatibility: if user knows it's transactions, the existing CSV flow should still work
- **Effort:** High (8-12 hours, significant refactor of import flow)
- **Dependencies:** C1, C3, C4, B4
- **Blocks:** Full unified user workflow

---

### **D. Frontend: TypeScript Bindings & Type Sync**

#### Gap D1: TypeScript Type Bindings for Preview Types
- **Status:** ⚠️ Partial (HoldingPreview exists, transaction preview types missing)
- **What:** Auto-generated TypeScript types from Rust via ts-rs
- **Where:** `frontend/src/bindings/` (auto-generated by `cargo test`)
- **Current state:** The following bindings already exist and are working:
  - `HoldingPreview.ts` - `{ account_id, symbol, sub_account, value, currency, as_of, status, existing_value }` (used by holdings dry_run)
  - `ImportResult.ts` - `{ rows_total, rows_inserted, rows_duplicate, filename, account_id, detected_bank, detection_confidence, errors }`
  - `ImportPayload.ts` - `{ account_id, transactions: ImportTransaction[] }`
  - `ImportTransaction.ts` - full transaction input type
  - `ImportRowError.ts` - `{ index, reason }`
  - `Holding.ts`, `HoldingType.ts`, `HoldingsSummaryResponse.ts`, etc.
- **What's missing:**
  - `TransactionImportPreview` type (the dryrun response from A2, once defined in Rust)
  - `TransactionPreviewRow` type (per-row preview with status)
  - Possibly a `HoldingsFileImportResponse` (from B3, if it differs from the existing JSON dryrun response)
- **What's Needed:**
  - Add `#[derive(TS)]` and `#[ts(export, export_to = "...")]` to new Rust types from A2
  - Run `cargo test` to regenerate bindings
  - Frontend types.ts already re-exports from `@/bindings/` (e.g., `frontend/src/types/models.ts:16` re-exports `ImportResult` from bindings)
- **Effort:** Low (< 1 hour, automatic once Rust types are defined)
- **Dependencies:** A2 (Rust types must exist first)
- **Blocks:** C2, C5

#### Gap D2: ImportResult Enhancement
- **Status:** ✅ NOT NEEDED as originally described
- **What was proposed:** Add `dry_run` and `preview` fields to ImportResult
- **Why this is wrong:** `ImportResult` represents the outcome of a COMMITTED import (total/inserted/duplicate counts). The dryrun response should be a SEPARATE type (`TransactionImportPreview`) with per-row detail. Conflating committed results with previews into one type leads to confusing optional fields and breaks the existing API contract. The holdings system already follows this pattern correctly: `import_holdings()` returns either a JSON object with `{ dry_run: true, preview: {...} }` OR `{ ok: true, holdings_imported: N }`, NOT a modified `ImportResult`.
- **Correct approach:** Keep `ImportResult` as-is for committed imports. Create `TransactionImportPreview` (A2) as the dryrun response type. The frontend uses the response's `dry_run` field (or inspects the response shape) to determine which view to render.
- **Effort:** None (no changes needed to ImportResult)
- **Dependencies:** None
- **Blocks:** None

---

### **E. Documentation & Specification**

#### Gap E1: Institutional Data Format Specifications (Bank/Brokerage Agnostic)
- **Status:** ❌ Not written
- **What:** Document the data formats that each supported institution can export, and how they map to Fynance data structures
- **Where:** `docs/guides/supported_formats/` directory (one file per institution family)
  - `docs/guides/supported_formats/banks_csv.md` (generic bank CSV, Revolut, Monzo, Lloyds, etc.)
  - `docs/guides/supported_formats/brokerages.md` (Trading 212, Freetrade, AJ Bell, etc.)
  - `docs/guides/supported_formats/pensions.md` (Vanguard, Fidelity, etc.)
  - `docs/guides/supported_formats/crypto_exchanges.md` (Kraken, Coinbase, etc.)
  - `docs/guides/supported_formats/custom_formats.md` (how to add new institutions)
- **What's Needed (per institution/format):**
  - Export options & where to find them in the institution's app/website
  - File format (CSV, Excel, PDF, JSON, OFX, etc.)
  - Example file snippet
  - Field mapping to Fynance structures:
    - Transaction fields: date, description, amount, currency, account, category, etc.
    - Holdings fields: symbol, name, quantity, price, value, currency, account, as_of date, etc.
  - Edge cases & quirks (pots, sub-accounts, multi-currency handling, fees, dividend formats)
  - Known limitations (e.g., "prices may be from EOD, not transaction time")
  - Confidence level (what % of data typically parses correctly)
- **Effort:** Medium (6-8 hours for initial set of common institutions)
- **Blocks:** None (informational, helps with B1 parser implementation)

#### Gap E2: Holdings Import API Documentation
- **Status:** ⚠️ Partial (endpoint exists, frontend usage not documented)
- **What:** Complete guide for importing holdings (manual + automated)
- **Where:** `docs/guides/holdings_import.md`
- **What's Needed:**
  - Manually formatting holdings JSON
  - Using dryrun preview
  - Understanding status field (new/modify)
  - Revolut export workflow (once B1 done)
  - Multi-currency handling
  - Sub-account/Pot support
- **Effort:** Low (2-3 hours)
- **Blocks:** None (informational)

#### Gap E3: Multi-Institution Unified Import Workflow Documentation
- **Status:** ❌ Not written
- **What:** Comprehensive guide for the new unified import workflow supporting any institution and dryrun preview
- **Where:** `docs/guides/unified_import_workflow.md`
- **What's Needed:**
  - **Conceptual overview:**
    - What is dryrun? Why use it? How does it work?
    - What institutions are supported? How to check?
    - What file formats are supported?
  - **Step-by-step workflow:**
    1. Export from your institution
    2. Upload file to Fynance
    3. System auto-detects institution & format
    4. Review detection results
    5. View transaction preview (new vs. duplicates)
    6. View holdings preview (new vs. modify)
    7. Choose to confirm or cancel
    8. Confirm imports data atomically
  - **Understanding the preview:**
    - What does "new" mean?
    - What does "duplicate" mean for transactions?
    - What does "modify" mean for holdings?
    - How is duplicate detection calculated (fingerprint)?
    - How are matching holdings identified?
  - **Error handling:**
    - What if institution detection fails?
    - What if file format is unsupported?
    - What if data validation fails?
    - How to manually specify format if auto-detection doesn't work
  - **Common tasks:**
    - Re-importing the same statement (idempotent)
    - Importing partial data (only transactions, only holdings)
    - Handling multi-currency accounts
    - Adding support for a new institution
  - **Troubleshooting:**
    - "I see duplicates that shouldn't be duplicates"
    - "Holdings aren't showing the correct values"
    - "The system detected the wrong institution"
- **Effort:** Low (3-4 hours)
- **Blocks:** None (informational)

---

## Implementation Priority & Sequence

### **Phase 1: Transaction Dryrun (4-5 days)**
Enable dryrun for CSV transaction imports (highest priority, unblocks other work)

1. A2: Define `TransactionImportPreview` and `TransactionPreviewRow` Rust types in model.rs
2. A3: Implement `dry_run_transactions()` in db.rs (fingerprint-based duplicate check)
3. A1: Add `dry_run` query param to `import_csv` and `import_json` handlers; wire up A3
4. D1: Run `cargo test` to generate TypeScript bindings for new types
5. C2: Add `dryRunImportCsv()` method to frontend ApiService
6. C1: Redesign PreviewTable to render per-row preview with NEW/DUPLICATE status; add Confirm button

**Deliverable:** Users can preview transaction imports before committing

---

### **Phase 2: Multi-Institution Holdings Extraction (6-8 days)**
Automatic holdings extraction from any supported financial institution

1. B1: Holdings parser framework + institutional implementations (Revolut, T212, Freetrade, generic CSV)
2. B2: Format auto-detection (which institution is the file from?)
3. B3: API endpoint for format detection & preview
4. E1: Document supported institution formats

**Deliverable:** Backend can automatically extract holdings from files exported by common institutions (banks, brokerages, pensions, etc.)

---

### **Phase 3: Holdings Frontend (4-5 days)**
UI for holdings import with dryrun

1. C5: API methods for holdings import + format detection
2. C3: Holdings upload UI (institution-agnostic file uploader)
3. C4: Holdings preview table
4. E2: Documentation

**Deliverable:** Users can upload holdings files from any institution and preview before committing

---

### **Phase 4: Unified Import Workflow (4-6 days)**
Single upload for both transactions & holdings from any institution

1. B4: Combined import endpoint (detects transactions + holdings + both)
2. C6: Unified import tab (works with any supported institution)
3. E3: Unified workflow documentation

**Deliverable:** Users can upload any export file from any supported institution → system auto-extracts transactions and/or holdings → shows preview of both → single confirm button

---

## Risk & Mitigation

| Risk | Impact | Mitigation |
|------|--------|-----------|
| **Institution format changes** | Parser breaks silently or produces wrong data | B2 (detection) includes confidence scoring; deprecate old parsers gracefully; add format versioning; alert users if detected confidence drops |
| **Duplicate detection false positives** | User thinks transaction is duplicate when it isn't (skips real import) | Use conservative fingerprint (date + amount + account); show preview of potential duplicates with context; allow user to override |
| **Duplicate detection false negatives** | User imports same transaction twice | Fingerprint is strong (SHA256 on date+amount+account); dryrun shows what will happen; document idempotency |
| **Holdings data mismatch** | User imports holdings but price/value is wrong or outdated | Document data freshness requirements per institution; show timestamp of extracted data; add warnings if data is >24h old |
| **Parser implementation errors** | New parser produces corrupt data; cascades to import | Unit test each parser extensively; run on test data before release; staging environment for new institutions |
| **User confusion (dryrun vs actual)** | User thinks they confirmed but didn't (or vice versa) | Crystal-clear UI: "Preview (not saved)" vs "Confirmed"; success page with import count; email/notification confirmation |
| **Performance (large files)** | UI freezes; backend times out; 100MB+ statements | Stream file processing; pagination in preview tables; async file processing; configurable max file size |
| **Atomic import failure** | Partial data commits if transaction+holdings import fails mid-way | Wrap B4 endpoint in DB transaction; rollback on any error; clear error message with rollback confirmation |
| **New institutions not in scope** | User tries to import from unsupported institution; system fails silently | B2 detection returns "unknown" with confidence <0.5; suggest manual format upload; document how to add new institution support |

---

## Testing Strategy

- **Unit tests:**
  - Each institutional parser (B1): test with sample exports from each institution
  - Format detection (B2): test confidence scoring with various file formats
  - Duplicate detection (A3): test fingerprint collisions, edge cases
  - Dryrun logic (A3, B3): ensure no writes to DB
  
- **Integration tests:**
  - End-to-end dryrun workflows (detect → preview → confirm → verify no writes)
  - Multi-institution test files (one file per institution)
  - Atomic import (both fail, transaction succeeds but holdings fails, vice versa)
  
- **Manual tests:**
  - Real export files from each supported institution (collect test data)
  - Re-import same file twice (verify idempotency)
  - Upload corrupted/partial files (verify error handling)
  - Large files (1000+ transactions/holdings)
  
- **Frontend tests:**
  - Preview UI (render large datasets)
  - Form submission (confirm → loading → success)
  - Error state (bad file, parse error, duplicate conflict)
  
- **Security tests:**
  - File upload: max size limits, format validation, malicious content
  - API: SQL injection in parser logic, path traversal

---

## Estimated Total Effort

| Phase | Days | FTE |
|-------|------|-----|
| 1: Transaction dryrun | 5 | 1 |
| 2: Holdings parser | 6 | 1 |
| 3: Holdings frontend | 5 | 1 |
| 4: Combined workflow | 4 | 1 |
| **Total** | **20** | **20 days** |

---

## Success Criteria

✅ User can upload export files from ANY supported financial institution (bank, brokerage, pension, etc.)
✅ System auto-detects institution, format, and data type (transactions and/or holdings)
✅ Format detection includes confidence score; low-confidence cases ask user for clarification
✅ Both transactions and holdings show comprehensive dryrun preview:
  - Transactions: new vs. duplicate (with fingerprint match shown)
  - Holdings: new vs. modify (with old value shown)
  - Both: all parsing warnings/errors highlighted
✅ User can review and click single "Confirm All" button
✅ Data commits atomically (all succeeds or all fails; no partial state)
✅ Dryrun calls never write to database (idempotent, repeatable)
✅ Re-importing same file twice is safe (fingerprints prevent duplicates)
✅ Comprehensive error messages guide users when issues arise
✅ New institutions can be added without code changes (pluggable parser architecture)
✅ At least 5 institutions supported in initial release (e.g., Revolut, T212, Freetrade, Monzo, generic CSV)

---

## Next Steps

1. **Prioritize phases:** Phase 1 (transaction dryrun) is the highest-value, lowest-risk improvement. It requires no new infrastructure, only extending the existing import path.
2. **Design decision for A2:** Should the CSV dryrun response include the full parsed `ImportPayload` so the confirm step can POST JSON (avoiding re-upload + re-parse)? Or should the frontend re-upload the file for confirmation? Tradeoff: carrying the payload is simpler UX but means the response can be large for big files.
3. **Collect test data:** Get real CSV exports from Trading 212, Freetrade, Revolut (holdings) to design B1 parsers against actual data.
4. **Architecture review:** The `HoldingsParser` trait (B1) should be a separate module from the existing `Importer` trait, since they serve different data types and have different output schemas.
5. **Scope decision:** Is the unified endpoint (B4) truly V0, or is it better to ship separate transaction dryrun + holdings file upload first and combine them later?

---

## Cross-Reference Findings (Research Audit, 2026-05-16)

This section documents discrepancies found between the original IMPORT_GAPS.md and the actual codebase state.

### Corrections Applied

1. **Gap A1 - Line numbers were approximately correct** but the description of what needs changing was imprecise. `import_json()` has NO query params at all (it only takes a JSON body); a new query struct or alternative mechanism is needed. The original doc implied just adding a field to `CsvImportQuery` would cover both endpoints.

2. **Gap A2 - Pseudo-code used invalid Rust syntax.** The original used inline anonymous structs (`pub preview: { ... }`) and string literal types (`"new" | "duplicate"`) which don't exist in Rust. Updated to describe the needed types conceptually without invalid syntax.

3. **Gap A3 - Understated complexity.** The original said "similar to dry_run_holdings" but missed a key difference: `dry_run_holdings()` checks by composite key `(account_id, symbol, sub_account, as_of)`, while transaction dryrun needs to compute fingerprints first. The fingerprint logic is embedded inside `Transaction::from_unified()` (importers/unified.rs:82) which constructs a full Transaction object. This means either: (a) build full Transaction objects just to get fingerprints, or (b) extract fingerprint computation into a standalone utility. The original doc didn't identify this design choice.

4. **Current State Table - "Format Detection" column was misleading.** Original said "✅ Monzo, Revolut, Lloyds (CSV)" implying hardcoded parsers. Reality: the LLM handles ALL formats dynamically. The three banks are what the LLM has been tested against, but it can parse any CSV with reasonable confidence. The detection is not hardcoded.

5. **Current State Table - "Multi-Institution Support" was understated.** Original said "⚠️ Limited (only 3 hardcoded)" but the LLM parser is inherently institution-agnostic for transactions. It returns a confidence score and will attempt any CSV. The "3 hardcoded" refers to the `BankFormat` enum used for display purposes, not a parsing limitation.

6. **Gap B1 - Overscoped for V0.** Original listed PDF, Excel, JSON, OFX as required V0 formats. For V0, CSV-only is realistic. Adding crate dependencies for PDF extraction (large, unreliable) and Excel (calamine) significantly increases binary size and maintenance burden. CSV covers the primary use case (Trading 212, Freetrade both export CSV).

7. **Gap C1 - Misidentified existing state.** Original said "PreviewTable exists but needs dryrun data" as if it's a simple data swap. Reality: PreviewTable renders AGGREGATE stats (three number cards + error table), not per-row data. It needs a complete redesign to render individual rows with status badges, not just a data format change.

8. **Gap C3 dependency was wrong.** Original listed "C2" (transaction API method) as a dependency for C3 (holdings upload UI). These are independent: C3 depends on B3 (backend holdings file endpoint), not C2.

9. **Gap D2 was misguided.** The original proposed adding `dry_run` and `preview` fields to `ImportResult`. This conflates two distinct response types. The holdings system already demonstrates the correct pattern: separate response shapes for dry_run vs. commit. The transaction system should follow the same pattern with a distinct `TransactionImportPreview` type.

10. **"Atomic Multi-Data Import" in Current State table was misleading.** Original said "❌ Missing (transactions only)" implying transactions aren't atomic either. Reality: individual transactions use `INSERT OR IGNORE` (each row independently succeeds or is a duplicate). The "atomicity" gap is specifically about cross-type consistency (importing transactions + holdings as one unit), which is a Phase 4 concern.

### Key Architecture Observations

1. **The LLM parser is the primary bottleneck for transaction dryrun UX.** Parsing a CSV via Claude Haiku takes 2-5 seconds. If the dryrun response includes the parsed payload (so confirm doesn't re-parse), the UX is: upload -> wait 3s -> see preview -> click confirm -> instant commit. If the confirm step re-uploads, it's: upload -> wait 3s -> see preview -> click confirm -> wait 3s again. Strong argument for carrying the payload in the dryrun response.

2. **Transaction fingerprint computation is cheap.** It's just SHA-256 of three strings. The expensive part is the LLM parse. Once transactions are parsed, checking fingerprints against the DB is O(n) individual SELECTs (or a single `WHERE fingerprint IN (...)` batch query). This means dryrun adds minimal overhead once parsing is done.

3. **Holdings parsers (B1) don't need LLM.** Brokerage CSVs have consistent headers. A simple header-detection + column-mapping approach works. This is much cheaper and faster than the transaction LLM path. Implementation is straightforward Rust CSV parsing.

4. **The existing `Importer` trait (mod.rs:19) is CLI-only.** It takes a `&Path` and a `&Db` reference and calls `import()` synchronously. The HTTP route handlers (`import_csv`, `import_json`) do NOT use this trait; they call the parser and DB directly. The trait is only used by the `fynance import` CLI subcommand. New work should extend the HTTP route handlers directly, not the `Importer` trait.

5. **Holdings dry_run response is untyped JSON.** The existing `import_holdings()` handler (holdings.rs:396) returns `serde_json::json!({...})` not a typed struct. This means the response shape isn't enforced by Rust's type system and there's no auto-generated TypeScript binding for the full dry_run response envelope. Only `HoldingPreview` (which goes inside the `preview.snapshots` field) has a binding. A typed response struct would improve both type safety and frontend code generation.
