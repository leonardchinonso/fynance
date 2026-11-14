# 30 - Income & tax V1: general income model, tax-document import, business income sync

Status: active. Feature branch: `feat/tax-return-v1` (one branch, one PR to `master`).
Supersedes the storage parts of plan 28 (the `employment_pay` table, its routes and its summary) and
fills in every "storage TBD" item in plan 29. Plan 29 stays the reference for the parse-pipeline
detail (prompt, validation, reconciliation maths, net-pay link suggestion, wizard UI); where the two
differ, this plan wins. Phase 2 (return pack, browser box-entry helper) is outlined at the end and is
not part of this build.

Public repo: synthetic data only. No real names, employers, businesses, figures or identifiers in
code, tests, fixtures, commits or PR text. "The owner" and "the partner" below are two taxpayer
profiles; "the shop app" is an external app that sells the partner's goods.

## 1. What we are building

1. A general **income model** per taxpayer profile: `income_sources` (employment, self-employment;
   property designed for, not built), `income_records` (one per period), `income_line_items`
   (line_type, amount, currency), and `income_checks` (a P60 or annual accounts kept as a
   reconciliation check, never summed). It replaces `employment_pay`.
2. A rebuilt **tax-year summary** per profile: employment per source (SA102 values), self-employment
   per source (SA103S values, including business-account expenses), interest (per profile, joint
   accounts split), Gift Aid, a CGT link, and per-source check status.
3. **Tax-document import** (payslips, P60s) through `/api/parse` → preview → commit (plan 29, now
   with real storage).
4. **Business income sync**: Fynance pulls monthly figures from the shop app's token-protected
   endpoint into self-employment records (the endpoint itself is a separate PR in the shop app's repo;
   see the private notes).
5. **RSU vest linking**: a payslip's vest line links to the matching `vest` investment event; the
   app shows whether the vest's cost basis equals the value taxed as pay and offers to align it.
6. The **Tax tab** rebuilt on the new model, still opt-in.
7. A **local-only CGT check** that the engine reproduces the filed 2024-25 CGT figures (no repo
   code required; findings stay outside the repo).

## 2. Decisions (settled with the owner; do not re-open)

- Income is per **profile**. Sources: `employment | self_employment | property | other`; property
  and other are accepted by the API but get no special summary or UI in V1.
- One **record per period** (a payslip month, a month of business sales). `period_start` and
  `period_end` are required. Records hold line items; each line type maps to a self-assessment box.
- **No year-end table of figures that gets summed.** A P60 or annual accounts is stored as an
  `income_check` (its printed figures plus documents) and only compared with the summed records. A
  genuine gap becomes one labelled **year-end adjustment** record (`kind = year_end_adjustment`),
  created by an explicit user action, visible in the record list.
- SA102 box 1 uses **taxable pay**, not headline gross. Per record: `taxable_pay` when present,
  else `gross_pay`. Both are stored.
- Tax-document import: profile required (UI defaults to the only non-default profile, i.e. when the
  user has exactly one profile besides the one they mainly use; if ambiguous, ask); unique net-pay
  match pre-filled as a removable chip.
- RSU: the vest value and its tax are payslip line items; company-stock disposals stay in the CGT
  engine. Sell-to-cover share sales are not payslip data.
- The Tax tab is hidden until there are income records or the Settings switch is on.

## 3. Decisions taken in this plan (reversible; flagged to the owner)

- **D1. `income_checks` table.** A P60's printed figures have to persist so the Tax page can show
  "matched" or "gap of X" later. They live in `income_checks`, which the summary never sums. This is
  the "reconciliation check", not a year-end data table.
- **D2. Business basis: order date as a cash-basis proxy.** The shop takes payment at checkout, so
  the order day is the day money is received; cash basis (the default for sole traders from
  2024-25) and order date differ only for refunds issued in a later month and platform payout
  timing. The shop app's Stripe ledger mirror was added after launch and has coverage gaps, so a true
  cash view from it is not reliable yet. The endpoint therefore reports by UK order day, splits
  April at the 6th so no period crosses a tax year, and lists refunds dated in a different month from
  their order (`late_refunds`) so Fynance can show them.
- **D3. Avoid double-counting business costs (needs the owner's confirmation, see Q1).** The shop
  app's materials, packaging and postage are per-order *costings*, while real purchases go out of
  the business bank account, which Fynance already holds. Default: from the shop app Fynance sums only
  **sales, refunds and platform-deducted costs** (card fees, marketplace fees, marketplace postage
  labels), which never appear as bank outflows. Materials, packaging, own postage and every other
  expense come from **categorised outflows on the source's business accounts**. The shop app's costing
  figures are stored as informational line types (shown, never summed). Changing this later is a
  data-table change (`summed` flag), not a schema change.
- **D4.** Joint-account interest is split equally between the account's profiles in the per-profile
  summary, labelled "joint, 1/N share".
- **D5.** Income amounts in a non-GBP currency convert at the record date's stored rate
  (`FxRateMap::convert_as_of`); a missing rate is a summary warning, not a 400.
- **D6.** The Tax-tab switch stays in localStorage (`fynance.showTaxTab`), as PR #136 built it.

## 4. Open question for the owner (build proceeds on the default)

- **Q1.** Are the partner's materials, packaging and own postage paid from the business bank
  account Fynance holds? **Default yes** (D3). If not, flip those line types to `summed` and drop the
  matching bank categories from the map.

## 5. Data model

Probe-based migration block "19" in `backend/src/storage/db.rs::migrate_schema` (pattern of block 18).
Also add the new tables to `db/sql/schema.sql` (they are new and fresh DBs should get them there; keep
the probe block for existing DBs). Money is TEXT Decimal, dates ISO, JSON id arrays as TEXT.

Drop `employment_pay`: if the table exists and `COUNT(*) = 0`, `DROP TABLE`; if it has rows, leave
it and `tracing::warn!` once (only dev DBs ran PR #137; it never reached `master` or the NAS). Delete
all employment-pay code, routes, docs entries, bindings and `backend/tests/employment_pay.rs`.

```sql
CREATE TABLE income_sources (
  id TEXT PRIMARY KEY,
  profile_id TEXT NOT NULL REFERENCES profiles(id),
  source_type TEXT NOT NULL,          -- employment | self_employment | property | other
  name TEXT NOT NULL,                 -- employer / business / property
  identifiers TEXT NOT NULL DEFAULT '{}',   -- JSON: employment {paye_reference?, payroll_id?};
                                            -- self_employment {description?, accounting_basis: "cash"}
  account_ids TEXT NOT NULL DEFAULT '[]',   -- self_employment: business bank accounts
  expense_category_map TEXT NOT NULL DEFAULT '{}', -- self_employment: {category_id: line_type}
  sync_kind TEXT,                     -- NULL | 'shop_app' (business income sync, section 8)
  active INTEGER NOT NULL DEFAULT 1,
  notes TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL
);
CREATE INDEX idx_income_sources_profile ON income_sources(profile_id);

CREATE TABLE income_records (
  id TEXT PRIMARY KEY,
  source_id TEXT NOT NULL REFERENCES income_sources(id),
  kind TEXT NOT NULL DEFAULT 'period',   -- period | year_end_adjustment
  period_start TEXT NOT NULL, period_end TEXT NOT NULL,
  pay_date TEXT,                      -- employment: payment date (PAYE is by payment date)
  tax_year TEXT NOT NULL,             -- derived server-side, never trusted from the client
  currency TEXT NOT NULL DEFAULT 'GBP',
  transaction_ids TEXT NOT NULL DEFAULT '[]',  -- optional links to bank credits
  document_ids TEXT NOT NULL DEFAULT '[]',
  origin TEXT NOT NULL DEFAULT 'manual',       -- manual | parse | shop_sync | adjustment
  external_ref TEXT,                  -- shop_sync: period key "YYYY-MM-DD..YYYY-MM-DD"
  notes TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL
);
CREATE INDEX idx_income_records_source_year ON income_records(source_id, tax_year);
CREATE UNIQUE INDEX idx_income_records_ext ON income_records(source_id, origin, external_ref)
  WHERE external_ref IS NOT NULL;

CREATE TABLE income_line_items (
  id TEXT PRIMARY KEY,
  record_id TEXT NOT NULL REFERENCES income_records(id) ON DELETE CASCADE,
  line_type TEXT NOT NULL,            -- see catalogue
  amount TEXT NOT NULL,               -- non-negative; direction comes from the line type
  currency TEXT NOT NULL,
  basis TEXT NOT NULL DEFAULT 'period',  -- period | year_to_date (YTD is never summed)
  label TEXT,                         -- as printed
  investment_event_id TEXT REFERENCES investments(id) ON DELETE SET NULL  -- rsu_vest_value only
);
CREATE INDEX idx_income_line_items_record ON income_line_items(record_id);

CREATE TABLE income_checks (
  id TEXT PRIMARY KEY,
  source_id TEXT NOT NULL REFERENCES income_sources(id),
  tax_year TEXT NOT NULL,
  check_type TEXT NOT NULL,           -- p60 | annual_accounts
  figures TEXT NOT NULL,              -- JSON [{line_type, amount, currency}]
  document_ids TEXT NOT NULL DEFAULT '[]',
  notes TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
  UNIQUE(source_id, tax_year, check_type)
);
```

Enable `PRAGMA foreign_keys` behaviour as the rest of the DB does (check `Db::open`); if FKs are not
enforced, do the `ON DELETE` work in Rust (delete line items with the record; null
`investment_event_id` when an investment event is deleted, in `delete_investment_event`).

Tax year rule (in Rust, `util/tax_year.rs`, add `TaxYear::for_date(NaiveDate)`):
employment → tax year of `pay_date`, else `period_end`; everything else → tax year of
`period_end`, and `period_start` must be in the same tax year (400 `period_crosses_tax_year`).
`year_end_adjustment` records span the whole tax year (6 Apr..5 Apr).

**Document references.** `income_records.document_ids` and `income_checks.document_ids` join
`list_documents`, `document_references` and `delete_document` (force strips ids) in `storage/db.rs`,
and `DocumentReferences` gains `income_records` and `income_checks`. **Transaction delete** strips the
id from `income_records.transaction_ids` (find every transaction delete path in `db.rs`).

## 6. Line-type catalogue (one Rust table, exported to the frontend)

`backend/src/income/line_types.rs`: `IncomeLineType` enum (snake_case serde) plus a static
`LineTypeInfo { line_type, source_type, label, summed: bool, sign: +1|-1, form, box_ref, note }`,
exposed at `GET /api/income/line-types`. Box numbers are for the 2024-25/2025-26 short forms; the
coder checks them against HMRC's published SA102 / SA103S notes and corrects the table if needed
(record the source URL in a comment).

Employment (`source_type = employment`):

| line_type | summed into | notes |
|---|---|---|
| `taxable_pay` | SA102 box 1 | preferred per record |
| `gross_pay` | SA102 box 1 only when the record has no `taxable_pay` | kept for display |
| `paye_tax` | SA102 box 2 | total PAYE for the period, includes tax on RSU |
| `employee_ni` | — (shown) | |
| `student_loan` | SA100 student loan repayments deducted by employer | |
| `postgraduate_loan` | SA100 same section | |
| `employee_pension` | — (shown) | |
| `rsu_vest_value` | — (inside taxable pay already; shown and checked) | may carry `investment_event_id` |
| `rsu_tax` | — ("of which" breakdown of `paye_tax`; never added again) | |
| `net_pay` | — (shown, used for bank-credit link) | |
| `other_payment`, `other_deduction` | — (shown) | |

Self-employment (`source_type = self_employment`):

| line_type | summed into | default `summed` |
|---|---|---|
| `sales` | SA103S turnover (+) | yes |
| `refunds` | SA103S turnover (−) | yes |
| `platform_fees` (card + marketplace fees deducted at source) | SA103S other allowable expenses | yes |
| `platform_postage` (marketplace postage labels deducted at source) | SA103S office/phone/postage costs | yes |
| `cost_of_goods` | SA103S cost of goods | yes (bank-categorised) |
| `postage` | SA103S office/phone/postage costs | yes (bank-categorised) |
| `packaging` | SA103S cost of goods | yes (bank-categorised) |
| `other_expenses` | SA103S other allowable expenses | yes (bank-categorised) |
| `costing_materials`, `costing_packaging`, `costing_postage`, `costing_other` | — (shop-app costings, info only, D3) | no |

Property types are reserved names only (`rent_received`, `property_expenses`), not built.

## 7. API (the integration contract; frontend codes to this)

All money as decimal strings. Register in `server/mod.rs`, document in `routes/docs.rs` and
`docs/api.html` (CI parity), ts-rs bindings for every type, committed. Remove the five
employment-pay routes.

```
GET    /api/income/line-types                       -> { items: [LineTypeInfo] }
GET    /api/income-sources?profile_id=              -> { items: [IncomeSource] }
POST   /api/income-sources                          -> 201 IncomeSource
PATCH  /api/income-sources/:id                      -> IncomeSource   (account_ids, expense_category_map, identifiers, name, active, notes)
DELETE /api/income-sources/:id                      -> 204; 409 source_has_records
GET    /api/income-records?profile_id=&source_id=&tax_year=  -> { items: [IncomeRecord] }  (line items embedded, sorted pay_date/period_end)
POST   /api/income-records                          -> 201 IncomeRecord  (line_items embedded)
GET    /api/income-records/:id                      -> IncomeRecord
PATCH  /api/income-records/:id                      -> IncomeRecord  (line_items present = replace all)
DELETE /api/income-records/:id                      -> 204
GET    /api/income-checks?profile_id=&tax_year=     -> { items: [IncomeCheck] }
POST   /api/income-checks                           -> 201 IncomeCheck; 409 on (source, year, type) clash
PATCH  /api/income-checks/:id   DELETE /api/income-checks/:id
POST   /api/income-sources/:id/year-end-adjustment  body {tax_year} -> 201 IncomeRecord;
       409 no_check | no_gap | adjustment_exists
POST   /api/income-records/import                   (crew A2) body TaxDocumentsImportPayload -> TaxDocumentsImportResult
POST   /api/income-sources/:id/sync                 (crew A3) body {from, to, dry_run} -> IncomeSyncResult
GET    /api/income/vest-candidates?record_id=       (crew A1) -> { items: [VestCandidate] }
GET    /api/tax/summary/:tax_year?profile_id=       -> TaxYearSummary (profile_id required: 400 missing_profile_id)
```

Validation (400s): unknown profile/source, bad dates, `period_end < period_start`,
`period_crosses_tax_year`, negative amount, line type not valid for the source type, `basis` other
than period/year_to_date, `investment_event_id` not a `vest` event, `transaction_ids` that do not
exist, invalid currency, empty PATCH body. Reuse the PATCH absent-vs-null pattern from PR #137.

**Year-end adjustment.** For employment: compare the check's `taxable_pay` (P60 "pay in this
employment") and `paye_tax` with the summed records (same box rules, `kind = period` only). If a
difference ≠ 0, create one record `kind = year_end_adjustment`, `origin = adjustment`, period = the
tax year, line items = the signed differences. Line items are otherwise non-negative; negative
amounts are allowed on `year_end_adjustment` records only (validated), and the docs say so. Same mechanics for
`annual_accounts` checks on self-employment (`sales` and total expenses).

**`TaxYearSummary`** (replace the PR #137 shape; keep `interest` and `donations` sub-shapes):

```
{ tax_year, start_date, end_date, profile_id,
  employment: [ { source_id, name, paye_reference?, record_count, months_covered: [YYYY-MM],
                  boxes: { pay, tax, student_loan, postgraduate_loan },      // SA102 values
                  shown: { gross_pay, employee_ni, employee_pension, rsu_vest_value, rsu_tax, net_pay },
                  check?: { check_id, pay, tax, pay_difference, tax_difference, status: matched|gap|adjusted },
                  rsu: [ { record_id, line_item_id, vest_value, investment_event_id?, event_value?, status: unlinked|matched|mismatch } ] } ],
  self_employment: [ { source_id, name, months_covered, turnover, expenses_by_line_type: {..},
                       total_expenses, net_profit, trading_allowance_better: bool,
                       bank_expenses: { mapped_total, unmapped: [{category_id, name, total}] },
                       info: { costing_* totals }, check?: {...} } ],
  interest: InterestSummary,      // per profile, joint accounts split (D4)
  donations: DonationSummary,     // per profile; joint accounts split the same way
  cgt_link: { start_date, end_date },   // frontend links to the existing CGT report
  warnings: [string] }            // missing fx rates, unknown NULL legs, etc.
```

Rules: sum only `kind in (period, year_end_adjustment)` and `basis = period` line items with
`summed = true`; checks are never summed (this fixes the PR #137 double count). Bank expenses: for a
self-employment source, outflows (`amount < 0`, ABS) in the tax-year window on `account_ids`,
`exclude_from_summary = 0`, not `internal_transfer`, grouped by category and mapped through
`expense_category_map`; unmapped categories are listed, not summed. Write explicit aggregate SQL with
Decimal sums in Rust (do not `SUM(CAST AS REAL)` new money legs). Trading allowance: £1,000 constant
in the line-type module; `trading_allowance_better = turnover > 0 && total_expenses < 1000`.

**RSU vest candidates.** For a record with an `rsu_vest_value` line: `vest` investment events of
accounts owned by the source's profile, dated within ±7 days of `pay_date` (else period). Return
`{investment_event_id, account_name, symbol, date, quantity, price_per_share, currency,
value_gbp}`. Status `matched` when the event's GBP value (qty × price, `convert_as_of` at vest date)
equals the line amount within £1; else `mismatch`. Aligning the cost basis is a frontend action via
the existing `PATCH /api/investments/:id` (set `price_per_share = vest_value / quantity` in the
event's currency, converted back at the vest-date rate) behind a confirm dialog; no new backend route.

## 8. Business income sync (crew A3, Fynance side)

- Config: `FYNANCE_SHOP_SYNC_URL` and `FYNANCE_SHOP_SYNC_TOKEN` (add to `.env.example`, commented,
  and `backend/RUNNING.md`). Missing → 400 `sync_not_configured`. Never log the token or the response
  body at INFO.
- `POST /api/income-sources/:id/sync {from: YYYY-MM-DD, to: YYYY-MM-DD, dry_run}`: source must be
  `self_employment` with `sync_kind = 'shop_app'`. `reqwest` GET `<url>?from=&to=` with
  `Authorization: Bearer`, timeouts like `importers/provider.rs`. Upstream non-200 → 502
  `sync_upstream_error` with status code only.
- Endpoint response contract (the shop-app PR implements exactly this):
  ```json
  { "basis": "order_date", "currency": "GBP", "generated_at": "...",
    "periods": [ { "start": "2025-04-06", "end": "2025-04-30",
      "sales_pence": 0, "refunds_pence": 0,
      "platform_fees_pence": 0, "platform_postage_pence": 0,
      "costing_materials_pence": 0, "costing_packaging_pence": 0, "costing_postage_pence": 0,
      "costing_other_pence": 0,
      "order_count": 0, "orders_missing_fee": 0, "orders_missing_costs": 0 } ],
    "late_refunds": [ { "order_month": "2025-03", "refund_date": "2025-04-10", "amount_pence": 0 } ] }
  ```
  Periods are calendar months, with April split at the 6th.
- Mapping: one record per period, `origin = shop_sync`, `external_ref = "start..end"`, pence → Decimal
  pounds, zero amounts omitted. Upsert by `(source_id, origin, external_ref)`: replace line items
  of an existing synced record; never touch `manual`/`parse` records. `dry_run` returns
  `{created, updated, unchanged, periods: [{external_ref, status, line_items}], late_refunds,
  warnings}` without writing; commit writes all in one SQLite transaction. Missing-fee/cost counts
  become warnings.
- Tests: a local `axum` test server (or `tokio::net::TcpListener` stub) serving canned JSON;
  cover auth header sent, upsert idempotence, April split, non-200, not configured, wrong source type.

## 9. Tax-document import (crew A2; plan 29 Part A with real storage)

Follow plan 29 Part A steps 1-12 with these changes:
- One line-type enum: plan 29's `TaxLineCode` is replaced by `IncomeLineType` (employment subset;
  `notional_pay` → `rsu_vest_value`, `tax_deducted` → `paye_tax`). Map in the validator.
- `TaxRecordStore` gets a real `Db` impl (duplicates against saved records by plan 29's key using
  sources' normalised names for the profile, `known_sources` from `income_sources`, linked transaction
  set from `income_records.transaction_ids`). Drop the `NoSavedRecords` metadata note.
- `store_parse_documents` takes `Option<&str>` account id (both existing callers pass `Some`).
- Commit route `POST /api/income-records/import` (all-or-nothing transaction, server-side duplicate
  skip): find-or-create the employment source by normalised name for the profile (store the PAYE
  reference when the draft has one; add `paye_reference: Option<String>` to the draft and the prompt);
  payslips → `income_records` (`origin = parse`, period + YTD line items, `transaction_ids =
  [draft.transaction_id]` when set); P60s → `income_checks` (`check_type = p60`), 409-style clash
  reported as a per-row error, not a failure of the batch.
- Provider: Anthropic only is required (OpenAI `NotSupported` stays a 400). Do not touch the OpenAI
  provider code.

## 10. Frontend

**B1, Tax page** (rebuild PR #136's page; first merge `feat/tax-year-view` into the crew branch):
- Keep: opt-in hook (swap "has records" to `listIncomeRecords`), Settings switch, nav filter, route,
  year picker shell, `CardState`, `InterestCard`, `GiftAidCard`, the null-vs-blank PATCH pattern.
- Profile: tax is per taxpayer. Use the navbar `?profile=`; when "All profiles", show a profile
  picker on the page (CGT report's preselect pattern, `cgt_report_page.tsx:66-89`).
- Sections: readiness checklist (per employment source: months covered and check status; per
  business: months synced, unmapped expense categories; interest; Gift Aid; CGT link) → Employment
  cards (one per source: monthly table with SA102 columns from the line-type catalogue, totals row,
  check row with "Create year-end adjustment" when status is `gap`, RSU sub-table: link picker over
  vest candidates, match status, "Align cost basis" confirm) → Self-employment cards (monthly table,
  "Sync from shop" with dry-run diff then confirm, late-refund notice, expense category mapping
  editor over the business accounts' categories, totals, trading allowance note, info costings
  collapsed) → Interest → Gift Aid.
- Dialogs: source add/edit (type, name, PAYE reference, business accounts, sync kind); record
  add/edit with a line-item editor filtered by source type; check add/edit.
- Service methods in `service.ts` / `real_service.ts` / `mock_service.ts` with mutation prefixes
  (`create|update|delete|import|commit|...`; name the sync call `importShopSync`), mock seed of
  synthetic data covering every section. Delete the employment-pay methods and hooks.
- Smoke: extend step 11a to open `/tax` in mock mode and assert an employment card, a check row and
  a business card on both viewports.
- Unit tests (vitest): pure helpers for table columns from the catalogue, totals, and the
  cost-basis alignment price maths.

**B2, import wizard**: plan 29 Part B as written, with commit going to
`POST /api/income-records/import` and the done screen linking to `/tax?profile=<id>`.

## 11. Crews (disjoint scopes; coders write code only and merge back into `feat/tax-return-v1`)

| Crew | Branch off `feat/tax-return-v1` | Scope (owns) | Leaves alone | Starts after |
|---|---|---|---|---|
| **A1 core backend** | `feat/income-core` | migration block, `schema.sql`, `backend/src/income/` (new: `mod.rs`, `line_types.rs`, `summary.rs`, `vest.rs`), `storage/db.rs` income + document/transaction-reference changes, `model.rs` income types **plus the A2/A3 contract types** (`TaxDocumentsImportPayload/Result`, `IncomeSyncRequest/Result`), `server/routes/income.rs` + `tax.rs` summary route, route registration, docs.rs/api.html for its routes, removal of all employment-pay code, `backend/tests/income_*.rs`, bindings | parse code, sync HTTP client, frontend source | — (first). Push a **contract commit** (types + bindings) before the rest |
| **A2 tax-doc parse** | `feat/tax-doc-parse` | plan 29 Part A files, `server/routes/income_import.rs`, its docs entries | income core storage except calling it; frontend | A1 merged |
| **A3 shop sync** | `feat/shop-sync` | `backend/src/income/sync.rs`, `server/routes/income_sync.rs`, `.env.example`, `RUNNING.md`, its docs entries, `backend/tests/income_sync.rs` | everything else | A2 merged (avoids docs/route-table conflicts) |
| **B1 Tax page** | `feat/tax-page-v2` | `frontend/src/pages/Tax*.tsx`, `pages/tax/` (new), `hooks/use_tax_tab.ts`, `hooks/data/use_tax.ts` (rename ok), service files (tax methods only), mock seed, smoke step 11a, settings switch | import pages, bindings, backend | A1 contract commit |
| **B2 import wizard** | `feat/tax-doc-ui` | plan 29 Part B files | Tax page files, backend | A2 contract types (in A1) and B1 merged (service files) |
| **C shop-app endpoint** | in the shop app's repo, see private notes | the shop app only | Fynance | any time (parallel) |

Check task (not a coder): **D CGT check**, local only, see private notes. Runs any time.

Coders: code and unit tests only; no full builds or reviewers (the owner's `cargo watch` may hold
the main checkout's binary; in a worktree, `cargo test --lib`/`clippy` are fine if the coder wants a
compile check). Merge back with a merge commit, report in under 15 lines: what changed, what is
untested, any contract deviation.

## 12. Review, checks, PRs

One Opus review of the whole `feat/tax-return-v1` diff against `master` (and the shop-app branch
against its base, same reviewer, separate fix list), run with:
backend `cargo fmt --check`, `cargo +1.85 clippy --all-targets -- -D warnings`, `cargo test`;
repo root `python scripts/check_api_docs.py`, `python scripts/check_pii.py`; frontend `npm test`,
`npx tsc --noEmit`, `npm run build`, smoke script in mock mode. Review focus: P60/check never summed,
taxable-vs-gross choice, YTD never summed, tax-year derivation at 5/6 April, document and transaction
reference integrity on delete, double-count of business costs (D3), sync idempotence, no secrets in
logs, no real data anywhere. One fix pass, then one PR `feat/tax-return-v1` → `master` (close PR #136
as superseded, linking the new PR).

## 13. Phase 2 (not this build)

- **Return pack**: `GET /api/tax/return-pack/:tax_year?profile_id=` producing (a) a printable PDF in
  the accountant's layout (SA100 summary, SA102 per employment, SA103S per business, SA108 from the
  CGT engine, interest, Gift Aid) and (b) a machine-readable box list `[{form, box, value, source}]`.
- **Browser box-entry skill**: first check HMRC's terms for SA online (and the Government Gateway
  terms) on automated entry. If allowed, a Claude-in-Chrome skill types the box list into the online
  return and stops before Submit; the owner submits. If not allowed, the skill only shows each value
  next to its box.
- Property income (SA105), US dividend withholding, true cash-basis business view from the shop
  app's money ledger once its coverage is complete.
