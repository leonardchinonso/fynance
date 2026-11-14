# 29 - Tax document import: payslips and P60s through the parse wizard

Status: active as the parse-pipeline reference; storage, commit route and line types are defined by plan 30 (plan 30 wins where they differ). Base branch: `feat/tax-return-v1` (integration). Depends on plan 28.

## Read this first

The owner is generalising the tax data model beyond employment (self-employment turnover and
expenses, rental income, all per taxpayer profile). Until that lands, this plan covers **only the
parse-pipeline integration**: how a "tax documents" output kind plugs into `POST /api/parse`, the
preview, document attachment, duplicate flagging, the bank-transaction link suggestion, and the
wizard UI. It targets an abstract **income record + line items** shape (defined below), not a
concrete table.

Everything that touches storage is marked **TBD** and is out of scope for both builders:

- the table(s) behind an income record, and the `employment_pay` migration (the earlier
  "split payslips and P60s into two tables" instruction is on hold, superseded by the
  generalisation);
- the commit route's path and its insert logic;
- the duplicate lookup against saved records, and the "already linked" transaction set;
- counting the new records as document references (`document_references`, `list_documents`,
  `delete_document` in `storage/db.rs`, plus `DocumentReferences` in `model.rs`);
- the Tax page (PR #136) and the tax-year summary.

Two requirements from the owner carry into whatever storage lands, and the preview already
follows them: **a P60 (year-end certificate) is never summed with payslips**, and the summary
rule is "per employer-year, use the P60 when one exists, else the sum of payslips, and always
show the payslips-vs-P60 difference so gaps (missing months) are visible".

What was read: plan 28; `server/routes/parse.rs`, `server/routes/employment_pay.rs`,
`server/mod.rs` (route table), `importers/document_parser.rs`, `importers/investments_parser.rs`,
`importers/pdf_parser.rs`, `importers/unified_parser.rs` (head), `importers/mod.rs`, the
`LlmProvider` trait, OpenAI's file stubs and the test `MockProvider` in `importers/provider.rs`,
`util/tax_year.rs`, the employment-pay and document-reference code in `storage/db.rs`, the
relevant `model.rs` types, `tests/parse_multipart.rs` (head), `.github/workflows/ci.yml`,
`docs/pii-guard.md`; frontend `pages/import/*`, `preview/preview_review.tsx`, `api/client.ts`,
`parseDocuments` in `api/real_service.ts` and `api/mock_service.ts`, `scripts/smoke_preview.mjs`
(import steps), and PR #136's diff (`feat/tax-year-view`). Not opened: `holdings_parser.rs` body,
`llm_parser.rs` body, `transactions_section.tsx` / `investments_section.tsx` bodies, `docs/api.html`
body, `check_api_docs.py` body. Assumed from names and call sites only.

## Goal

The owner drops UK payslips and P60s (PDF or photo) into the import flow. The app reads each one
(many payslips per PDF is fine), shows a review table (employer, tax year, period, gross, tax, NI,
student loan, net, flags), suggests the bank credit that matches each payslip's net pay, and on
commit saves records with the source documents attached. Nothing is written until the user
commits.

## Decisions taken (cheap to reverse unless noted)

1. **Extend `/api/parse`, do not add a parse route.** `ReturnType` gains `tax_documents`. It is
   exclusive: combined with transactions/holdings/investments it is a 400 `invalid_return_type`.
   Mirrors the investments addition (third kind on `ReturnType`, own parser, own preview section).
2. **Tax documents ignore `mode`.** Split/unified is a statement concern. The tax path makes one
   LLM call per file, in parallel, through `chat_with_files_and_tools` (exact per-file source
   attribution, smaller outputs, fewer truncations). `experimental.agent` and `.auth` are honoured.
3. **No account; a profile instead.** Tax documents belong to a taxpayer, not a bank account. The
   multipart takes `profile_id` (required when `tax_documents` is true; see question Q1);
   `account_id` is ignored for this kind. Documents are stored with `account_id = NULL`,
   origin `"parse"`.
4. **File cap 15 for tax documents** (a year of monthly payslips plus a P60), 5 for everything
   else. The cap check moves after the multipart loop (hints can arrive after files); a hard cap of
   15 still applies inside the loop. Body limit (50 MB) unchanged.
5. **Extraction is line items, not fixed columns.** The LLM returns a header per document plus
   `line_items: [{code, label, amount, basis}]`, with `basis` = `period` or `year_to_date`. This
   maps onto any "income record + line items" storage and keeps RSU/notional pay, pension and YTD
   figures without new columns. The UI derives its fixed columns from the `period` items.
6. **Tax year is computed in Rust, never trusted from the model.** Payslip: the tax year
   containing the pay date (PAYE is by payment date), falling back to `period_end`, then
   `period_start`. P60: the tax year ending on the printed "year to 5 April YYYY". A printed year
   that disagrees with the derived one is a warning, and the derived one wins.
7. **Which "pay" figure reconciles against a P60:** a payslip's `taxable_pay` when printed, else
   `gross_pay`. A P60's "pay in this employment" is extracted as `taxable_pay`. Both figures are
   kept as line items; storage decides what it calls gross (see Q2).
8. **RSU / equity:** the vest value is a `notional_pay` line item (it is inside gross and taxed,
   but never paid in cash). The matching after-tax offset deduction is not extracted separately.
   Tax on equity stays inside `tax_deducted` (total PAYE for the period). The row gets a warning
   "includes notional pay X". Sell-to-cover share sales are not payslip data and are ignored.
9. **Arithmetic check is a warning, never an error:** `gross - tax - NI - student loan -
   postgraduate loan - employee pension - other deductions + other payments - notional pay` vs
   `net`, tolerance 0.01, skipped when gross, tax or net is missing. Pension schemes and benefits
   make this unreliable, so it only flags.
10. **Duplicates are flagged, never inserted twice.** The key is `(profile_id, income_kind,
    normalised source_name, tax_year, document_type, period_end or pay_date)`; P60 keys omit the
    date. Name normalisation: lowercase, strip punctuation, collapse whitespace, drop a trailing
    `ltd|limited|plc|llp|inc`. Within one preview a repeat gets `same_key_as_row`. Against saved
    records the lookup is behind a trait (storage TBD). The commit route re-checks and skips
    duplicates server-side regardless of what the client sends.
11. **Net-pay link suggestion:** candidates are transactions with `amount == +net_pay` (Decimal
    compare in Rust, not text compare), same currency, date within **plus or minus 4 days** of the
    pay date, in accounts belonging to the profile (all accounts if none match), not already
    linked (TBD set) and not already suggested to an earlier row in the same preview (assigned
    greedily by date distance). Up to 3 candidates per row. Exactly one candidate: it is
    pre-filled in the row's draft `transaction_id`, and the UI shows it as an accepted chip the
    user can remove. Several: nothing pre-filled, the user picks. P60 rows never get candidates.
12. **No separate `payload` object in the preview.** Each preview row carries its editable `draft`,
    and the commit body is `{ profile_id, documents: draft[] }` built from the rows the user keeps.
    This avoids the preview-row to payload-index mapping that `payload_index.ts` exists for.
    Duplicate and error rows are unticked by default.
13. **Providers:** the parser uses only `chat_with_files_and_tools` and a tool schema, so Anthropic
    and Gemini work unchanged. OpenAI currently returns `NotSupported` for file input; that maps
    to 400 `provider_not_supported`, as it does today for PDFs. OpenAI file support is out of scope
    (another builder is adding `FYNANCE_OPENAI_BASE_URL`; do not touch the OpenAI provider).
14. **Synthetic fixtures only.** Invented employer (`Example Widgets Ltd`), round figures, no NI
    number at all (or the reserved `QQ` prefix, which the PII guard's issuing rules never match),
    no names, no addresses.

## Open questions (recommended answer in bold; "yes" accepts all)

- **Q1. Should a tax-document parse require a profile?** **Yes:** `profile_id` required, the UI
  defaults to the only non-default profile, else asks. Only the owner knows whether records will
  be per profile; the coordinator said they will be.
- **Q2. What does storage call "gross pay" for a payslip: taxable pay or total gross?**
  **Taxable pay when printed, else total gross**, because that matches the P60 and SA102 box 1.
  This is a storage-mapping choice, so it can wait for the storage design; the pipeline keeps
  both.
- **Q3. Pre-fill a unique net-pay match, or require an explicit click?** **Pre-fill** (decision 11)
  so twelve payslips are not twelve clicks; the chip is removable. Say no if a deliberate click
  is preferred; it is a one-line change in the backend and a default in the UI.

## The pipeline contract (both builders code to this)

### Request: `POST /api/parse` (multipart, unchanged route)

| Field | Tax documents |
|---|---|
| `files[]` | 1 to 15 files: `.pdf`, `.png`, `.jpg`, `.jpeg`, `.webp`, `.gif`. CSV/XLSX are rejected with 400 `unsupported_tax_document_format`. |
| `hints` | `{"return_type":{"transactions":false,"holdings":{"enabled":false},"investments":false,"tax_documents":true},"hint":<string or null>}`. `experimental.mode` ignored. |
| `profile_id` | **New field.** Required for tax documents: 400 `missing_profile_id`; unknown id is 400 `profile_not_found`. Ignored for other kinds. |
| `account_id` | Ignored for tax documents (still required for every other kind). |
| `parse_id` | Unchanged; progress events use `task_id = "<filename>:tax_documents"`. |

New 400 codes: `invalid_return_type` (tax_documents combined with another kind),
`too_many_files` (over 15 for tax, over 5 otherwise), `missing_profile_id`, `profile_not_found`,
`unsupported_tax_document_format`, `response_truncated` (a file's call hit `max_tokens`; same
message style as the unified path).

### Response: `IngestionPreview` gains one optional field

`transactions`, `holdings`, `investments` are present and empty (count 0) for a tax parse;
`metadata.institution_detected` is null; `documents` lists the stored files as today.

Rust types (all in `backend/src/model.rs`, `#[derive(Debug, Clone, Serialize, Deserialize, TS)]`,
`#[ts(export, export_to = "../../frontend/src/bindings/")]`, money as
`#[serde(with = "rust_decimal::serde::str")] #[ts(type = "string")]`):

```rust
// IngestionPreview: new field. Option + skip + ts(optional) so existing frontend code that
// builds IngestionPreview literals (mock_service.ts) keeps compiling.
#[serde(default, skip_serializing_if = "Option::is_none")]
#[ts(optional)]
pub tax_documents: Option<TaxDocumentIngestionResult>,

// ReturnType: new field, same reasoning (ts-rs 10 only allows optional on Option fields).
#[serde(default, skip_serializing_if = "Option::is_none")]
#[ts(optional)]
pub tax_documents: Option<bool>,
// plus ReturnType::wants_tax_documents(&self) -> bool { self.tax_documents.unwrap_or(false) }
// and is_valid() also accepts tax_documents alone.

#[serde(rename_all = "snake_case")]
pub enum TaxDocumentType { Payslip, AnnualCertificate }      // annual_certificate = P60

#[serde(rename_all = "snake_case")]
pub enum IncomeKind { Employment }   // the generalisation adds self_employment, property, ...

#[serde(rename_all = "snake_case")]
pub enum TaxLineCode {
    GrossPay, TaxablePay, TaxDeducted, EmployeeNi, StudentLoan, PostgraduateLoan,
    EmployeePension, NotionalPay, NetPay, OtherPayment, OtherDeduction,
}

#[serde(rename_all = "snake_case")]
pub enum AmountBasis { Period, YearToDate }   // a P60's figures are basis = period

pub struct TaxLineItem {
    pub code: TaxLineCode,
    pub label: Option<String>,   // as printed, e.g. "RSU notional"; ts(optional)
    pub amount: Decimal,         // non-negative; direction comes from the code
    pub basis: AmountBasis,
}

/// The editable record a preview row proposes, and exactly what commit receives.
pub struct TaxDocumentDraft {
    pub document_type: TaxDocumentType,
    pub income_kind: IncomeKind,
    pub source_name: String,            // employer (later: business / property)
    pub tax_year: String,               // YYYY-YY, derived in Rust
    pub pay_date: Option<String>,       // YYYY-MM-DD; None for P60
    pub period_start: Option<String>,   // YYYY-MM-DD
    pub period_end: Option<String>,
    pub currency: String,               // ISO 4217, default GBP
    pub line_items: Vec<TaxLineItem>,
    pub transaction_id: Option<String>, // accepted net-pay credit
    pub document_ids: Vec<String>,
    pub notes: Option<String>,
}

pub struct NetPayCandidate {
    pub transaction_id: String,
    pub account_id: String,
    pub account_name: String,
    pub date: String,                   // YYYY-MM-DD
    pub amount: Decimal,
    pub currency: String,
    pub description: String,
    pub days_from_pay_date: i64,        // signed: negative = before pay date
}

pub struct TaxDocumentPreviewRow {
    pub index: usize,
    pub status: TransactionPreviewStatus,   // new | duplicate | error (existing enum)
    pub error_reason: Option<String>,       // ts(optional)
    pub existing_id: Option<String>,        // saved record it duplicates (TBD store); ts(optional)
    pub same_key_as_row: Option<usize>,     // ts(optional)
    pub row_confidence: f32,
    pub source_document_ids: Vec<String>,
    pub warnings: Vec<String>,
    pub net_pay_candidates: Vec<NetPayCandidate>,
    pub draft: TaxDocumentDraft,
}

pub struct TaxReconciliation {
    pub source_name: String,                // as shown on the first row of the group
    pub tax_year: String,
    pub currency: String,
    pub certificate_row: Option<usize>,     // the P60 row index, if uploaded
    pub payslip_rows: Vec<usize>,
    pub payslips_pay_total: Decimal,        // taxable_pay, else gross_pay, per payslip
    pub payslips_tax_total: Decimal,
    pub certificate_pay: Option<Decimal>,
    pub certificate_tax: Option<Decimal>,
    pub latest_ytd_pay: Option<Decimal>,    // from the latest payslip's YTD items
    pub latest_ytd_tax: Option<Decimal>,
    pub pay_difference: Option<Decimal>,    // certificate_pay - payslips_pay_total
    pub tax_difference: Option<Decimal>,
    pub months_covered: Vec<String>,        // YYYY-MM of each payslip pay date, sorted
    pub message: String,                    // one plain-English line for the banner
}

pub struct TaxDocumentIngestionResult {
    pub count: usize,
    pub new: usize,
    pub duplicate: usize,
    pub errors: usize,
    pub profile_id: String,
    pub rows: Vec<TaxDocumentPreviewRow>,
    pub reconciliation: Vec<TaxReconciliation>,
    pub known_sources: Vec<String>,         // existing employer names for this profile (TBD store)
}
```

`Option<Decimal>` money fields use `rust_decimal::serde::str_option` with
`#[ts(optional, type = "string")]`, like `EmploymentPay`.

### Commit (contract fixed now, route path and storage TBD)

```rust
pub struct TaxDocumentsImportPayload { pub profile_id: String, pub documents: Vec<TaxDocumentDraft> }
pub struct TaxDocumentImportError { pub index: usize, pub reason: String }
pub struct TaxDocumentsImportResult {
    pub inserted: usize,
    pub duplicates: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")] #[ts(optional)]
    pub errors: Vec<TaxDocumentImportError>,
}
```

Provisional route: `POST /api/tax-documents/import`, all-or-nothing in one SQLite transaction for
valid rows, server-side duplicate skip, `transaction_id` must exist. **Part A defines these types
only; the route is added with the storage work.** The frontend mocks it.

## Part A: backend pipeline (one builder)

Branch `feat/tax-doc-parse-backend` off `feat/tax-return-v1`.

**Owns:**
- `backend/src/importers/tax_documents/` (new): `mod.rs`, `parser.rs` (tool schema + one call per
  file), `validate.rs` (raw tool output to `TaxDocumentPreviewRow`), `reconcile.rs`, `link.rs`
  (net-pay candidate assignment), `store.rs` (the storage seam).
- `backend/config/prompts/tax_document_parser.txt` (new).
- `backend/src/importers/mod.rs` (register module), `backend/src/importers/document_parser.rs`
  (`ReturnType` field and `is_valid`), `backend/src/server/routes/parse.rs`,
  `backend/src/model.rs` (types above), `backend/src/util/tax_year.rs` (`TaxYear::for_date`),
  `backend/src/storage/db.rs` (one new read query, nothing else).
- `backend/tests/tax_document_parse.rs` (new), `backend/tests/fixtures/tax/` (new).
- `backend/src/server/routes/docs.rs` and `docs/api.html` (the `/api/parse` entries only),
  `CLAUDE.md` (the `/api/parse` line in the REST list).
- `frontend/src/bindings/*` (regenerated by `cargo test`, commit them).

**Leaves alone:** the `employment_pay` table, its routes and summary, document-reference counting,
the OpenAI provider, unified/split statement code paths, any other frontend file.

**Steps (tests first where marked):**

1. **Contract commit first.** Add the model types and `ReturnType.tax_documents`, run
   `cargo test --lib` to regenerate bindings, push. This commit unblocks Part B. Put
   `tax_documents: None` in the one `IngestionPreview` literal in `document_parser.rs`.
2. **Test first:** `TaxYear::for_date(NaiveDate) -> TaxYear` in `util/tax_year.rs`: 5 April maps to
   the year ending that day, 6 April starts the next, 29 Feb 2024 is in 2023-24.
3. **Storage seam** in `store.rs`:
   ```rust
   pub trait TaxRecordStore {
       fn existing_record_id(&self, key: &TaxRecordKey) -> anyhow::Result<Option<String>>;
       fn known_sources(&self, profile_id: &str) -> anyhow::Result<Vec<String>>;
       fn linked_transaction_ids(&self) -> anyhow::Result<HashSet<String>>;
   }
   pub struct NoSavedRecords;   // returns None / empty; used by the route until storage lands
   ```
   `TaxRecordKey` holds the normalised fields from decision 10. While `NoSavedRecords` is in use,
   the route adds the metadata note "Duplicate check against saved tax records is not available
   yet." The storage work replaces it with a `Db` impl.
4. **Prompt** `tax_document_parser.txt`. Must say: one array element per payslip or P60 (many per
   PDF); `document_type` `payslip`, `p60` or `other` (other covers P45, P11D, letters: emitted
   as an error row "not a payslip or P60"); amounts as plain decimal strings with no currency
   symbol or thousands separator, never negative; record both "this period" and "year to date"
   figures when printed, with the right basis; P60: use "in this employment" figures, never
   "previous employment"; RSU/share/benefit lines are `notional_pay`, do not record their offset
   deduction; `tax_deducted` is total PAYE for the period; dates `YYYY-MM-DD`; copy the printed
   tax year text verbatim into `printed_tax_year`; never invent a figure that is not printed; a
   `row_confidence` per element and a `detection_confidence` per file.
5. **Test first, `parser.rs` + `validate.rs`:** tool name `parse_tax_documents`; raw shape
   `{ detection_confidence, documents: [{ document_type, employer, pay_date?, period_start?,
   period_end?, printed_tax_year?, currency?, line_items: [{code, label?, amount, basis}],
   row_confidence, notes? }] }`. Unit tests (using `crate::importers::provider::testing::MockProvider`
   with canned tool JSON) cover: two payslips in one file give two rows with the same
   `source_document_ids`; tax year derived from pay date across the 5/6 April boundary; P60 year
   from printed year end; printed year disagreeing gives a warning; `£1,234.50` style amounts
   are cleaned or rejected (decide: strip `£`, `,`, spaces; anything else is an error row);
   negative amount is an error row; missing employer, missing period gross/taxable pay or missing
   tax deducted is an error row; `row_confidence` under `FYNANCE_IMPORT_MIN_ROW_CONF` (default
   0.70) is an error row; notional pay gives the warning; arithmetic mismatch gives a warning and
   status stays `new`; `document_type: other` is an error row; a `max_tokens` stop reason is
   surfaced as `response_truncated`.
6. **Test first, `reconcile.rs`:** groups by (normalised name, tax_year, currency). Cases: P60 plus
   12 payslips that match gives difference 0; P60 plus 11 payslips gives a non-zero difference and
   `months_covered` with a gap; payslips only gives no certificate fields and the latest YTD;
   P60 only gives no payslip totals. P60 figures never enter `payslips_*_total`.
7. **Test first, `link.rs`:** pure function over `(rows, candidates, linked_ids)`. Cases: exact
   amount inside the window is matched; amount off by 0.01 is not; outside the window (5 days)
   is not; two payslips with the same net and two credits are assigned one each by date
   distance; an already linked transaction is excluded; a unique match pre-fills
   `draft.transaction_id`; P60 rows get none.
8. **DB read** in `storage/db.rs`: `find_credit_transactions(from: NaiveDate, to: NaiveDate,
   account_ids: Option<&[String]>) -> Result<Vec<NetPayCandidate>>` with
   `days_from_pay_date` left 0 for `link.rs` to fill. SQL filters date range
   (`T00:00:00`..`T23:59:59`) and joins `accounts` for the name; Rust parses `amount` as Decimal
   and keeps positives. One query covering the whole preview's date span, not one per row.
   Never log descriptions at INFO.
9. **Route** in `parse.rs`: read the new `profile_id` field; move the file-cap check after the
   loop (decision 4); branch on `hints.return_type.wants_tax_documents()` before the
   `account_id` requirement; validate exclusivity, profile, formats; store documents with a
   `store_parse_documents(db, &documents, None)` (change its `account_id` parameter to
   `Option<&str>`; the two existing callers pass `Some(&account_id)`); run the per-file calls in a
   `JoinSet` like `extract_all_parallel`; then, holding the DB lock, run validate, the
   within-batch keys, the store lookups, the credit query, link and reconcile; cost entries use
   `parser_call_cost("tax_documents", &call)`; emit the same progress phases as split mode.
10. **Route tests** in `backend/tests/tax_document_parse.rs` (no LLM reachable, like
    `parse_multipart.rs`): tax plus transactions is 400 `invalid_return_type`; missing
    `profile_id` is 400 `missing_profile_id`; unknown profile is 400 `profile_not_found`; a CSV is
    400 `unsupported_tax_document_format`; 15 PDFs pass the cap and reach provider config, 16 are
    `too_many_files`; 6 statement files are still `too_many_files` (existing test must stay
    green); no `account_id` needed for tax.
11. **Fixtures** `backend/tests/fixtures/tax/`: `make_fixtures.py` (stdlib only, writes a minimal
    text PDF) producing `synthetic_payslips.pdf` (two monthly payslips plus one P60 for
    `Example Widgets Ltd`, one payslip with a notional RSU line), plus a `canned_tool_output.json`
    the unit tests load. No image fixture is needed: images take the same provider path as PDFs. A `#[ignore]` live test in
    `tests/tax_document_live.rs` gated on the provider key, mirroring `gemini_live.rs`, runs the
    PDF through the real parser and asserts 3 rows.
12. **Docs:** `/api/parse` in `routes/docs.rs` OpenAPI (new `profile_id` form field,
    `tax_documents` in hints, `tax_documents` in the response, new error codes) and the same in
    `docs/api.html`; one-line update to the `/api/parse` comment in `CLAUDE.md`. No new routes in
    this part, so `check_api_docs.py` should stay green; run it anyway.

**Done when:** a live parse of the synthetic PDF returns three rows with correct tax years and a
reconciliation entry; all tests above pass; bindings committed; the checks below are green.

## Part B: frontend (one builder)

Branch `feat/tax-doc-parse-frontend` off `feat/tax-return-v1`, **after PR #136 is merged** (it
edits `service.ts`, `real_service.ts`, `mock_service.ts`). Rebase onto Part A's contract commit as
soon as it is pushed; until then put the TS shapes from the contract in
`frontend/src/types/tax_import.ts` with a comment to delete it once `bindings/` has them.

**Owns:**
- `frontend/src/pages/import/import_landing.tsx`, `frontend/src/pages/import/import.tsx`,
  `frontend/src/pages/import/file_upload.tsx`, `frontend/src/pages/import/recent_imports_list.tsx`.
- `frontend/src/pages/import/tax/` (new): `tax_import.tsx`, `tax_preview_review.tsx`,
  `tax_documents_table.tsx`, `reconciliation_banner.tsx`, `net_pay_link_cell.tsx`,
  `tax_drafts.ts`, `tax_drafts.test.ts`.
- `frontend/src/hooks/use_recent_imports.ts`.
- `frontend/src/api/service.ts`, `frontend/src/api/real_service.ts`, `frontend/src/api/mock_service.ts`.
- `frontend/scripts/smoke_preview.mjs`.

**Leaves alone:** `frontend/src/bindings/` (generated), `Tax.tsx`, `preview_review.tsx` and the
statement sections, every backend file.

**Steps:**

1. **Service methods** (names start with `parse`/`commit`, so `client.ts` cache invalidation
   already covers them):
   ```ts
   /** Stage 1 for tax documents. POST /api/parse with tax_documents hints and profile_id. */
   parseTaxDocuments(files: File[], profileId: string, hint: string | null, opts?: ParseOptions): Promise<IngestionPreview>
   /** Stage 2. Provisional POST /api/tax-documents/import (storage TBD). */
   commitTaxDocuments(payload: TaxDocumentsImportPayload): Promise<TaxDocumentsImportResult>
   ```
   Real: same `FormData` + SSE code as `parseDocuments` (factor the shared body into a private
   helper rather than copying it). Mock: scripted progress like the statement mock, then a
   synthetic preview: two payslips and one P60 for `Example Widgets Ltd`, one payslip with a
   notional-pay warning and a unique pre-filled net-pay candidate, one duplicate row, one
   reconciliation entry with a non-zero difference. `commitTaxDocuments` mock returns counts.
2. **Test first, `tax_drafts.ts` (vitest):** `periodAmount(draft, code)` and
   `setPeriodAmount(draft, code, value)` (edit or insert a `basis: "period"` item, never touch
   YTD items); `defaultKept(row)` (true only for `new`); `buildCommitPayload(result, rows, kept)`
   (keeps drafts in preview order, drops unkept); `taxYearForDate("2026-04-05")` is `2025-26`,
   `"2026-04-06"` is `2026-27` (wrap the existing `ukTaxYearForDate(date: Date)` in
   `api/cgt_filter_params.ts`, parsing the ISO string as a local date; test both boundaries).
3. **Landing:** third card "Import tax documents" ("Payslips and P60s, PDF or photo. Review, then
   save to your tax records.") routing to `/import/tax`.
4. **`import.tsx`:** when the path segment is `tax`, render `<TaxImport />` before the "No
   accounts found" guard and the account-based step logic. No other change to the statement flow.
5. **`file_upload.tsx`:** add `variant?: "statement" | "tax"` (default `statement`). `tax` hides the
   return-type checkboxes and period select, accepts `.pdf,.png,.jpg,.jpeg,.webp,.gif`, shows
   "Up to 15 files" and enables submit when at least one file is chosen. Header text comes from
   the existing `accountName`/`accountInstitution` props (pass the profile name and "Tax
   documents").
6. **`tax_import.tsx`:** URL-derived steps like `import.tsx`: `/import/tax` is setup (profile
   select, defaulting to the only non-default profile, plus `FileUpload variant="tax"` and the
   optional context hint), `?entry=<id>` is preview, `?done=1` is the summary. Parse errors show
   in the same destructive banner; progress uses `ParseProgressController` and
   `ReloadingOverlay` like the statement flow.
7. **Recent imports:** add optional `kind?: "statement" | "tax"` and `profileId?: string` to
   `RecentImportEntry` (absent means statement). Statement lists filter tax entries out; the tax
   setup step lists only tax entries. `recent_imports_list.tsx` must not crash on an entry with
   no matching account.
8. **`tax_preview_review.tsx` + `tax_documents_table.tsx`:** heading "Review tax documents";
   `MetadataHeader` reused; `reconciliation_banner.tsx` above the table (one line per entry from
   `message`, amber when a difference is non-zero, green when zero); table columns: keep
   checkbox, Type (Payslip / P60 badge), Employer (input with a datalist of `known_sources`), Tax
   year, Period / pay date, Gross pay, Tax, NI, Student loan, Net pay (all editable via
   `setPeriodAmount`), Status (reuse `status_badge.tsx`; show `same_key_as_row` like
   transactions), Bank credit (`net_pay_link_cell.tsx`: accepted chip with remove, or a picker
   over `net_pay_candidates` showing date, account, amount and day offset), Source chips (reuse
   `components/source_chips.tsx`), warnings icon with tooltip. Row expand shows all line items
   including YTD. Edits persist to the recent-imports cache like `PreviewReview` does.
   Desktop and mobile layouts both usable (the smoke script runs both).
9. **Commit:** "Save N records" opens `components/confirm_dialog.tsx`; on success show a summary
   (inserted, duplicates skipped) with a link to `/tax`; on error keep the inline error banner
   pattern from `PreviewReview`.
10. **Smoke script:** add a tax pass for both viewports: `/import`, click "Import tax documents",
    pick profile if a select is shown, `setInputFiles` with a small buffer named
    `synthetic_payslips.pdf` (mock mode does not read it), wait for "Review tax documents", assert
    the "Gross pay" header, the reconciliation banner text, a "P60" badge, remove and re-accept
    the bank-credit chip, submit, confirm, wait for the summary. Screenshots to `.playwright-mcp/`.

**Done when:** the full flow works in mock mode on both viewports, `npm test`, `tsc` and build pass,
and against a live backend with Part A merged a real parse of the synthetic PDF renders (commit
will 404 until storage lands; that is expected and should show the error banner cleanly).

## Checks

Backend (from `backend/`; do not start a manual build if the owner's `cargo watch` is running,
per their workflow; `cargo test --lib` and clippy do not relink the running binary):
```
cargo fmt --check
cargo +1.85 clippy --all-targets -- -D warnings      # CI pins 1.85; rustup toolchain install 1.85 if missing
cargo test
cargo test --test tax_document_parse
FYNANCE_PARSE_PROVIDER=anthropic cargo test --test tax_document_live -- --ignored   # needs a key
```
Repo root: `python scripts/check_api_docs.py`, `python scripts/check_pii.py` (the fixtures
generator and canned JSON are text and are scanned; the PDF is not).

Frontend (from `frontend/`): `npm test`, `npx tsc --noEmit`, `npm run build`, then with
`npm run dev` up, `node scripts/smoke_preview.mjs` and look at the tax screenshots in
`.playwright-mcp/`.

Manual end to end (live, local default DB only, never the X: dataset): start the backend, upload
`backend/tests/fixtures/tax/synthetic_payslips.pdf` via `/import/tax`, confirm three rows,
2025-26 style tax years derived from pay dates, the notional-pay warning, and a reconciliation
line. To see a link suggestion, first create a synthetic credit via `POST /api/transactions/import`
with the fixture's net amount near its pay date.

## Merge order

PR #136 into `feat/tax-return-v1`, then Part A, then Part B (rebased on A). CI only gates PRs to
`master`, so run the checks above locally on each branch before merging into the integration
branch. The storage work (TBD) then adds the commit route, the `TaxRecordStore` impl for `Db`,
document-reference counting and the Tax page changes.

## Risks and where to look hardest

- **Gross vs taxable pay and YTD vs period confusion** is the most likely real-data error. Review
  the prompt wording and the reconciliation maths together; a wrong basis silently doubles a year.
- **RSU payslips:** check that notional pay is not counted twice (as gross and again as
  `other_payment`) and that the arithmetic warning does not fire on every equity payslip.
- **Tax-year derivation at 5/6 April** and payslips dated in the next tax year for work done in
  the previous one (pay date wins; make sure the UI shows the pay date so the user sees why).
- **Link suggestion false positives:** a regular transfer of the same amount near payday. Exact
  amount plus window plus "not already linked" limits it; the chip is removable.
- **`store_parse_documents` signature change** touches the two statement paths; existing parse
  tests must stay green.
- **Moving the file-cap check after the loop** means up to 15 files are buffered before a
  statement request is rejected; acceptable under the 50 MB body limit, but verify the existing
  `six_files_rejected_as_too_many` test still passes.
- **Documents look orphaned** until the storage work counts the new records as references. A
  force-delete in that window would not unlink anything because there is nothing saved yet;
  after storage lands it must. Flag this in the storage plan.
- **Public repo:** reviewers read every fixture, test literal and prompt example for real names
  or figures. The PII guard does not catch employers or salary amounts.
