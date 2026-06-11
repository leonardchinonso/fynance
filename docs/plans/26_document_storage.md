# Document Storage & Per-Row Source Provenance

Status: **Implemented** (branch `feat/document-storage`). Supersedes the V5
"Document Storage" sketch in [20_post_v0_plans.md](20_post_v0_plans.md); that
section can be replaced with a pointer here.

## Why

Before this change, files uploaded through `POST /api/parse` were discarded
after extraction, so there was no way to look at a transaction, holding, or
investment and answer "which document produced this?". The V5 sketch hung
provenance off `import_log` (one document per import, transactions only), which
could not express the two real cases we have today: a single account import is
often several files at once (positions CSV + transaction history, or multiple
screenshots), and holdings/investment imports do not write `import_log` rows at
all. So documents became a first-class entity and provenance is recorded
**per row, per file**.

## What shipped

### Data model

A `documents` table ([db/sql/schema.sql](../../db/sql/schema.sql)) storing
metadata (filename, on-disk path, mime type, size, `content_hash`, `origin`,
optional `account_id`, `uploaded_at`). `content_hash` is unique, so re-uploading
or re-parsing the same bytes reuses the existing row (the main defence against
orphan files piling up across a session). `origin` is `"parse"` (auto-stored
during a parse) or `"manual"` (standalone upload) and is informational only.

Each of `transactions`, `holdings`, `investments` gained a
`source_document_ids` JSON-array TEXT column, mirroring the existing
`accounts.profile_ids` pattern (reverse-queried with `json_each`). Added to the
`CREATE TABLE`s and to `migrate_schema` for existing DBs (guarded `ALTER TABLE`,
existing rows default to `'[]'`).

Files live on disk in a `documents/` subdir beside the SQLite DB
(`<id>_<sanitised_filename>`), created `0700`, written once, never a SQLite BLOB.

### Document API

New `routes/documents.rs`, registered in
[server/mod.rs](../../backend/src/server/mod.rs), auth mirroring the import
endpoints (loopback browser passes; non-loopback needs a bearer token):

```
GET    /api/documents                 # list + reference_count + orphaned flag
GET    /api/documents/:id             # metadata
GET    /api/documents/:id/download    # stream raw bytes
POST   /api/documents                 # standalone upload (multipart) → origin=manual
DELETE /api/documents/:id             # 409 if referenced
DELETE /api/documents/:id?force=true  # unlink from every row, then delete
```

`reference_count` and the delete checks are computed with `json_each` across the
three tables. Documented in [docs/api.html](../../docs/api.html) (CI coverage
check passes) and the OpenAPI spec in
[docs.rs](../../backend/src/server/routes/docs.rs).

A document is **orphaned** when `reference_count == 0`, regardless of `origin`
(this covers both an uncommitted parse and a standalone upload linked to
nothing). `DELETE` without `force` returns `409 document_referenced` with a
per-entity `references` breakdown so the UI can confirm precisely; with
`?force=true` it strips the id from every referencing row in one transaction,
then removes the row and file.

### Per-row attribution (in V1, not deferred)

Each extracted row is attributed to the specific file it came from, via a
transient `source_file` carrier on the extraction rows:

- **Split mode**: `extract_all_parallel`
  ([document_parser.rs](../../backend/src/importers/document_parser.rs)) already
  extracts per file, so each row is stamped with its `doc.filename` at merge
  time. Exact, no model involvement.
- **Unified mode**: a `source_file` field was added to the unified tool schema
  and the prompt
  ([config/prompts/unified/output_shape.txt](../../backend/config/prompts/unified/output_shape.txt)),
  so the model tags each row with its originating filename.
- **Resolution**: the parse route stores each file first (deduped by hash),
  builds a `filename → document_id` map, and `build_multi_preview` resolves each
  row's `source_file` to `source_document_ids`. Fallbacks: exactly one document
  → that id; unmatched/missing with multiple documents → all call documents plus
  a `metadata.notes` entry. Rows are never dropped.

`source_document_ids` was added to the commit payload types (`ImportTransaction`,
`Holding`, `CreateInvestmentEventBody`), so the ids ride through the parse →
preview → commit flow unchanged, including the frontend's localStorage cache.
On commit the column is persisted; on a duplicate (transactions/investments
fingerprint, holdings upsert identity) the incoming ids are **unioned** into the
existing row so re-imports keep the audit trail complete.

### Frontend

A "Documents" card on the Reports landing and a `/reports/documents` page
([frontend/src/pages/reports/documents/documents_page.tsx](../../frontend/src/pages/reports/documents/documents_page.tsx)):
table of all documents (filename, type, size, origin, account, upload date,
link count), per-row download and delete, an amber **Orphaned** badge, a
standalone upload button, and a force-delete confirm dialog that quotes the
reference breakdown. Wired through the api client with `DocumentReferencedError`
carrying the breakdown, plus mock-mode support. The smoke script
([smoke_preview.mjs](../../frontend/scripts/smoke_preview.mjs)) gained a
documents step.

## Deliberately deferred (fast follow)

- **Surfacing `source_document_ids` on holdings/investments read endpoints.** It
  is persisted for all three types and the Documents page reference counts /
  delete work fully, but only `GET /api/transactions` returns the array today
  (cheap: two SELECTs). The holdings/investments read mappings default it to
  empty to avoid touching six-plus SELECTs. The "Source" icon column on the
  Portfolio / Investments tables is the natural follow-up that needs this.
- **Documents CLI** (`fynance document list|delete --force`).
- **Per-file precision note copy**: the fallback note exists; richer UI surfacing
  of partial attribution is a polish item.

## Verification

Backend: `cargo clippy --all-targets -- -D warnings` and `cargo test` green
(documents storage unit tests cover hash dedup, cross-table reference counts,
delete-409-when-referenced, force-unlink, and origin-independent orphan
detection). Live route verified end to end: a freshly built backend serves
`GET /api/documents` as `200 application/json`. Frontend: `tsc --noEmit` +
`npm run build` green; the Documents page (table, orphan badge, force-delete
confirm) verified in mock mode via Playwright with no console errors.

## Out of scope

Document versioning/mutation (create + delete only), OCR / re-parsing, content
dedup beyond exact-bytes hash, and encryption at rest (same threat model as the
unencrypted DB).
