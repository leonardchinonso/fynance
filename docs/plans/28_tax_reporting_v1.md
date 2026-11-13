# 28 - Tax Reporting V1: PAYE employment records, Gift Aid, tax-year summary

Status: superseded by plan 30 (storage model, routes, summary). Kept for history. Branch: `feat/tax-return-v1` (integration). Feature branches: `feat/employment-pay-api` (backend), `feat/tax-year-view` (frontend).

## Why

The app today supports CGT only. A full UK self-assessment return (see the SA100/SA102/SA108/SA302 shape) additionally needs gross pay and tax deducted under PAYE (never visible as bank movements), untaxed interest per account, and Gift Aid donations. Transactions stay pure cash events; gross-pay detail lives in a new linked table. `donation_taxable` / `donation_non_taxable` category types already exist and become the Gift Aid flag; `interest_taxable` / `interest_non_taxable` transactions already carry the interest data. No new transaction columns.

## Data model

New table (probe-based migration in `backend/src/storage/db.rs`, money as TEXT/Decimal, dates ISO):

```sql
CREATE TABLE employment_pay (
  id TEXT PRIMARY KEY,                -- uuid v4
  tax_year TEXT NOT NULL,             -- "2025-26" style: YYYY-YY, second = (first+1) mod 100
  period_start TEXT,                  -- ISO date, nullable (P60/annual rows have none)
  period_end TEXT,
  employer TEXT NOT NULL,
  is_p60 INTEGER NOT NULL DEFAULT 0,  -- annual summary row
  gross_pay TEXT NOT NULL,
  tax_deducted TEXT NOT NULL,
  employees_ni TEXT,                  -- NULL = unknown, distinct from 0
  student_loan TEXT,
  net_pay TEXT,
  currency TEXT NOT NULL DEFAULT 'GBP',
  transaction_id TEXT REFERENCES transactions(id) ON DELETE SET NULL,
  document_ids TEXT NOT NULL DEFAULT '[]',  -- JSON array, mirrors transaction document refs
  notes TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_employment_pay_tax_year ON employment_pay(tax_year);
```

No new category types. UK tax year: 6 April to 5 April, `tax_year "2025-26"` = 2025-04-06..2026-04-05. Put the parser/formatter in `backend/src/util/` (shared, unit-tested).

## API contract (this is the integration contract - frontend codes to this exactly)

All money renders as decimal strings (same convention as existing endpoints). Routes registered in `server/routes/mod.rs`, documented in `docs/api.html` and the OpenAPI block in `routes/docs.rs` (CI parity check).

```
GET    /api/employment-pay?tax_year=YYYY-YY   # optional filter; sorts period_end asc, NULLs last
  -> { "items": [EmploymentPay] }
POST   /api/employment-pay                    # body: EmploymentPayPayload
  -> 201 EmploymentPay
PATCH  /api/employment-pay/:id                # partial update
  -> EmploymentPay
DELETE /api/employment-pay/:id                # 204
GET    /api/tax/summary/:tax_year             # :tax_year = YYYY-YY
  -> TaxYearSummary
```

`EmploymentPay` = all table fields (ints as bool for `is_p60`; NULLs as omitted/optional). `EmploymentPayPayload` = table minus `id`/`created_at`/`updated_at`; require `tax_year`, `employer`, `gross_pay`, `tax_deducted` >= 0, valid dates, valid `tax_year` format (reject otherwise, 400). `transaction_id` must exist if provided (400 otherwise). `PATCH` ignores unknown/absent fields.

```rust
TaxYearSummary {
  tax_year: String,
  start_date: String, end_date: String,            // ISO, the 6 Apr..5 Apr window
  employment: EmploymentSummary {
    gross_pay, tax_deducted, employees_ni, student_loan, net_pay,   // sums, NULL legs treated as 0 with has_unknown flag
    payslip_count: u32, has_p60: bool,
  },
  interest: InterestSummary {
    taxable_total, non_taxable_total,
    by_account: [{ account_id, account_name, taxable, non_taxable }],  // only accounts with interest in range
  },
  donations: DonationSummary { gift_aid_total, non_gift_aid_total },   // donation_taxable / donation_non_taxable outflows in range
}
```

Interest/donation legs use preferred-currency conversion through the existing `FxRateMap` path, same as other aggregate endpoints. Rows with `exclude_from_summary = 1` are excluded. Do NOT reuse the buggy `get_transactions_by_category` (fixed in PR #134, not yet live): write dedicated aggregate SQL with explicit `SUM()`.

ts-rs: derive TS for all new response types so `frontend/src/bindings/` regenerates.

## Frontend

New "Tax" tab (`frontend/src/pages/Tax.tsx` + route/nav entry), reading through `src/api/client.ts`:

1. Tax-year picker (current tax year + any years present in data).
2. Readiness checklist card: rows = Employment (payslip count, P60 present Y/N), Interest (accounts covered), Gift Aid (total or nil). Status chips ok/needs-attention. Follow the ingestion-checklist feel, not a blocking form.
3. Employment table: one row per `employment_pay` item + totals footer; add/edit via modal form (all payload fields; transaction link optional text/select; P60 toggle). Delete with confirm.
4. Interest card: per-account taxable/non-taxable totals.
5. Gift Aid card: gift-aid vs non-gift-aid totals, with a hint that the flag is the `donation_taxable` category type.

Until the backend branch merges and bindings regenerate, frontend defines the shapes in `src/types/tax_v1.ts` (legacy types dir is allowed) with a comment to delete once `bindings/` has them. Keep it plain Tailwind/shadcn, no new chart types. Update `frontend/scripts/smoke_preview.mjs` only if it breaks (new nav tab appears in the header - if the script asserts nav items, teach it).

## Tests

- `backend/tests/employment_pay.rs`: CRUD round-trip, decimal strings preserved, tax_year filter, 400s on bad payload, transaction_id validation, summary sums over synthetic interest (two accounts, two currencies with fx) + donation types + `exclude_from_summary` respected, P60 row counted once, employees_ni NULL vs 0.
- util unit tests for tax-year math (leap, boundary days 5/6 April).
- `server_smoke` route registration as existing pattern.

## Phasing / tonight's scope

- Tonight: backend table+endpoints+summary+tests+docs, frontend Tax page, both PRs green on CI, merged into `feat/tax-return-v1`.
- Later (not tonight): built-in payslip/P60 PDF parsing (external agents can already POST records via this API), return-pack PDF generation, US dividend withholding records, CGT validation against the filed 2024-25 return, live data entry through the API once merged.

## Non-negotiables

No real names/employers/figures anywhere in repo, tests, commits or PRs (synthetic only). cargo fmt + clippy pinned toolchain + `python scripts/check_api_docs.py` before push. Frontend: tsc + build.
