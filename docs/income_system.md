# How the income system works

This page explains how Fynance records income and turns it into the figures you copy onto a UK Self
Assessment return. The first half is for anyone using the app. The second half (database, code
pointers) is for contributors. All names and numbers below are made up.

The design decisions live in `docs/plans/30_income_and_tax_v1.md` (income model) and
`docs/plans/29_tax_document_import.md` (reading payslips and P60s). This page describes what is
merged on master, which is not always the same as the plans.

## The short version

1. You add an **income source**: an employer, or a small business.
2. You record **income records** against it, one per pay period (a payslip) or per trading period
   (a month of sales). Each record holds **line items**: taxable pay, PAYE tax, sales, fees and so on.
3. You can add a **check**: the year's official total (a P60 for an employer, annual accounts for a
   business). Fynance compares the check with the sum of your records and tells you if they differ.
4. The **Tax page** adds the records up per tax year and shows the figure for each Self Assessment box.

The one rule that shapes everything: **a figure is only ever counted once.** Annual totals (P60,
annual accounts) and running year-to-date totals are used to check the monthly records. They are
never added to them.

## What is built, and what is not

| Part | State on master |
|---|---|
| Income sources, records, line items, checks (API and database) | Built |
| Tax page: employment and self-employment cards, interest and Gift Aid cards, readiness list | Built |
| Year-end adjustment button and API | Built |
| Parsing payslips and P60s from PDFs or photos (`return_type.tax_documents` on the parse endpoint) | Built (backend) |
| Committing reviewed payslips and P60s (`POST /api/income-records/import`) | Built (backend) |
| Import wizard screens for payslips and P60s | **Not built.** The frontend has the generated types only; nothing in `frontend/src/pages/import/` handles tax documents yet |
| Shop app sync | Built. `POST /api/income-sources/:id/sync`, used by the "Sync from shop" button and dialog. Needs `FYNANCE_SHOP_SYNC_URL` and `FYNANCE_SHOP_SYNC_TOKEN` (see "Shop sync" below) |
| Property income | Reserved. The line types exist; no summary is computed |

Until the import wizard lands, payslips and P60s are entered by hand in the Tax page dialogs, or sent
to the API directly.

## Words used

- **Tax year**: 6 April to 5 April, written `2025-26`.
- **Taxable pay**: pay that income tax is charged on. This is what SA102 box 1 wants.
- **Gross pay**: pay before any deductions. Not the same as taxable pay, because pension
  contributions taken before tax lower taxable pay.
- **Year to date (YTD)**: the running total printed on every payslip ("so far this tax year").
- **P60**: the employer's end-of-year summary of pay and tax.
- **Annual accounts**: the business's own year-end totals.

## The data model

Four tables, all in `db/sql/schema.sql`. Money is stored as text decimals (never floating point).
Lists of ids are JSON text.

```
profiles (the taxpayer)
   |
   +-- income_sources         one employer or one business
          |
          +-- income_records        one period (a payslip, a month of sales)
          |      |
          |      +-- income_line_items   the amounts inside a record
          |
          +-- income_checks         one P60 or annual-accounts figure set per year (never summed)
```

### income_sources

```sql
CREATE TABLE income_sources (
    id TEXT PRIMARY KEY,
    profile_id TEXT NOT NULL REFERENCES profiles(id),
    source_type TEXT NOT NULL,        -- employment | self_employment | property | other
    name TEXT NOT NULL,
    identifiers TEXT NOT NULL DEFAULT '{}',   -- JSON: paye_reference, payroll_id, ...
    account_ids TEXT NOT NULL DEFAULT '[]',   -- self_employment: the business bank accounts
    expense_category_map TEXT NOT NULL DEFAULT '{}', -- self_employment: {category_id: line_type}
    sync_kind TEXT,                   -- NULL | 'shop_app'
    active INTEGER NOT NULL DEFAULT 1,
    notes TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL
);
```

A source belongs to one profile. `account_ids` and `expense_category_map` are how a business source
finds its costs in bank data (see "Business costs" below). The API rejects them, and `sync_kind`,
on any other source type.

### income_records

```sql
CREATE TABLE income_records (
    id TEXT PRIMARY KEY,
    source_id TEXT NOT NULL REFERENCES income_sources(id),
    kind TEXT NOT NULL DEFAULT 'period',   -- period | year_end_adjustment
    period_start TEXT NOT NULL,
    period_end TEXT NOT NULL,
    pay_date TEXT,                         -- employment only
    tax_year TEXT NOT NULL,                -- worked out by the server, never typed in
    currency TEXT NOT NULL DEFAULT 'GBP',
    transaction_ids TEXT NOT NULL DEFAULT '[]',  -- optional links to bank credits
    document_ids TEXT NOT NULL DEFAULT '[]',     -- the uploaded payslip files
    origin TEXT NOT NULL DEFAULT 'manual', -- manual | parse | shop_sync | adjustment
    external_ref TEXT,                     -- shop_sync: "YYYY-MM-DD..YYYY-MM-DD"
    notes TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL
);
```

`origin` says where the record came from. A unique index on `(source_id, origin, external_ref)`
means a sync can be re-run without duplicating a period.

### income_line_items

```sql
CREATE TABLE income_line_items (
    id TEXT PRIMARY KEY,
    record_id TEXT NOT NULL REFERENCES income_records(id) ON DELETE CASCADE,
    line_type TEXT NOT NULL,     -- see backend/src/income/line_types.rs
    amount TEXT NOT NULL,        -- not negative, except on year_end_adjustment records
    currency TEXT NOT NULL,
    basis TEXT NOT NULL DEFAULT 'period',   -- period | year_to_date
    label TEXT,
    investment_event_id TEXT REFERENCES investments(id) ON DELETE SET NULL  -- rsu_vest_value only
);
```

`basis` is the important column. A payslip prints both "this month" and "so far this year". Both can
be stored, but only `period` lines are ever added up. `year_to_date` lines are kept so the
parse preview can compare them with the P60 and nothing else reads them.

### income_checks

```sql
CREATE TABLE income_checks (
    id TEXT PRIMARY KEY,
    source_id TEXT NOT NULL REFERENCES income_sources(id),
    tax_year TEXT NOT NULL,
    check_type TEXT NOT NULL,    -- p60 | annual_accounts
    figures TEXT NOT NULL,       -- JSON [{line_type, amount, currency}]
    document_ids TEXT NOT NULL DEFAULT '[]',
    notes TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
    UNIQUE(source_id, tax_year, check_type)
);
```

One check per source, year and type. Its figures are **never summed with records**. They are the
answer key the records are marked against.

### What is never summed

- `year_to_date` line items.
- Checks (P60, annual accounts).
- Any line type the catalogue marks `summed = false`. The catalogue is a table in
  `backend/src/income/line_types.rs`, served at `GET /api/income/line-types`, and the UI builds
  its columns from it. Examples: employee NI, employee pension, net pay, RSU vest value and RSU tax
  (both already inside taxable pay and PAYE), and the shop app's per-order `costing_*` lines.

## From documents to boxes

```
 payslip / P60 files (PDF or photo)
        |  parse (LLM reads the pages)             backend/src/importers/tax_documents/parser.rs
        v
 preview rows: validate, flag duplicates,          validate.rs, mod.rs
 suggest the bank credit for net pay,              link.rs
 compare payslips with the P60                     reconcile.rs
        |  you review and confirm
        v
 POST /api/income-records/import                   routes/income_import.rs
   payslip -> income_records (origin parse) + income_line_items
   P60     -> income_checks (type p60)
        |
        v
 GET /api/tax/summary/:tax_year                    income/summary.rs
   sums period lines -> SA box figures
   compares checks with the sums -> matched | adjusted | gap
        |
        v
 Tax page cards
```

### Reading documents (backend only today)

The parse endpoint, given `return_type.tax_documents`, accepts up to 15 PDFs or images for one
profile and returns preview rows. Per row, the server:

- Maps the model's codes to line types, cleans amounts (drops currency signs and commas, rejects
  negatives), and turns any row missing a date, a taxable or gross pay figure, or low in
  confidence into an error row.
- Works out the tax year itself from the pay date. A printed year on the page only produces a
  warning if it disagrees. A P60 takes its year from the printed year end.
- Forces every P60 line to `period` basis (a P60 is already an annual figure).
- Marks a row a duplicate if the same employer (name compared ignoring case, punctuation and a
  trailing company suffix: Ltd, Limited, PLC, LLP or Inc) already has that payslip month or that P60 year, or if it repeats an earlier
  row in the same batch.
- Suggests the bank credit that matches the net pay, looking within 4 days of the expected pay
  date, and never one already linked to another record.
- Builds a reconciliation per employer and year: payslip totals against the P60, with the latest
  YTD figures shown beside them. A positive difference means the P60 is higher, usually because a
  month is missing.

Parsing needs an LLM provider to be configured. Nothing is saved until the import call.

### Committing

`POST /api/income-records/import` writes valid rows in one transaction and reports bad rows one by
one without failing the batch. A payslip becomes a `period` record with origin `parse`, creating
the employer source if needed. A P60 needs taxable pay and PAYE tax figures and becomes a `p60`
check. A second payslip for the same period, or a second P60 for the same year, is not written.

### The tax year rule

Set in `backend/src/income/validate.rs`:

- Employment records belong to the tax year of their **pay date** (PAYE is by payment date), or the
  period end if there is no pay date. A payslip for a period ending 3 April but paid on 7 April
  lands in the next year.
- Everything else belongs to the tax year of its **period end**. A business or year-end-adjustment
  record may not span 5/6 April; the server refuses it. Split it into two records.

### Employment: SA102

For each employment source and tax year, only `period` lines of `period` and `year_end_adjustment`
records are added, per line type.

| Box | Figure | How it is built |
|---|---|---|
| SA102 box 1 | Pay | Per record, **taxable pay if the record has it, otherwise gross pay**. Then summed |
| SA102 box 2 | Tax taken off | PAYE tax |
| SA100 student loan section | Student loan, postgraduate loan | Summed (the SA100 wording was not verified against the form) |

Shown beside the boxes but not added to them: gross pay, employee NI, employee pension, RSU vest
value, RSU tax, net pay.

Example with made-up numbers. Three payslips each print gross 3,000, pension 150, taxable 2,850 and
tax 400. Box 1 is 8,550 (taxable, not gross) and box 2 is 1,200. The third payslip also prints
"taxable pay to date 8,550". That YTD figure is stored but ignored, otherwise the total would be
counted twice.

RSUs: a vest shows up in the payslip as taxable pay and PAYE already, so `rsu_vest_value` and
`rsu_tax` are informational. The Tax page compares each vest line with the matching vest event
in the investments data (values within a small tolerance) and shows a match badge.

### Self-employment: SA103S

| Box | Figure |
|---|---|
| 9 | Turnover: sales minus refunds |
| 11 | Cost of goods, packaging |
| 18 | Postage, platform postage |
| 19 | Platform fees, other expenses |

The card shows turnover, total expenses by line type and net profit. It also shows a hint when
turnover is positive but total expenses are under the 1,000 trading allowance, because then claiming
the allowance instead of actual costs could be better.

Total expenses are the expense lines on records **plus** categorised bank outflows (next section).

### Business costs: the double-count rule

A shop app knows the materials, packaging and postage each order used, but those are estimates
("costings"). The real money for them left the business bank account, which Fynance already has.
Adding both would count every cost twice.

So the rule is:

- From a shop app, only **sales, refunds and costs the platform takes at source** (card fees,
  marketplace fees, marketplace postage labels) are summed. These never show up as bank outflows.
- Materials, own postage, packaging and everything else come from **bank outflows** on the
  source's business accounts, grouped by transaction category and turned into SA103S lines through
  the source's `expense_category_map`.
- The shop app's per-order figures are stored as `costing_materials`, `costing_packaging`,
  `costing_postage`, `costing_other`: shown as information, never summed.
- Outflows in a category with no mapping are listed as "unmapped" with their totals, so a cost is
  never silently dropped. The "Map expense categories" dialog assigns them.

The shop sync follows this rule: sales, refunds, platform fees and platform postage are summed, and the `costing_*` lines are information only. The line-type table has the
`summed` flag for exactly that reason, so the rule can change without a schema change.

### Checks and the year-end adjustment

A check compares a year's official figure with the records.

- Employment compares taxable pay (or gross if that is all the P60 has) and PAYE tax.
- Self-employment compares turnover and total expenses (record lines plus mapped bank outflows).

Status:

| Status | Meaning |
|---|---|
| `matched` | The check equals the sum of the ordinary period records |
| `gap` | They differ and nothing explains it yet (often a missing month) |
| `adjusted` | They differ, but a year-end adjustment record closes the whole gap |

When there is a gap, the card offers **Create year-end adjustment**
(`POST /api/income-sources/:id/year-end-adjustment`). It writes one record of kind
`year_end_adjustment`, origin `adjustment`, holding the signed differences (the only record type
allowed negative amounts). Employment gets taxable pay and PAYE tax lines. A business gets a sales
line and an other-expenses line. After that the summary includes the adjustment in the box totals, and
the check reads `adjusted`. The check itself is still never added: only the adjustment is.

Typical reason for an adjustment: an employer corrects pay in the final payslip of the year, or
a month of payslips was lost. Prefer adding the missing payslip if you have it.

## Shop sync

`POST /api/income-sources/:id/sync` pulls monthly figures from the shop app for a source whose
`sync_kind` is `shop_app`. It needs `FYNANCE_SHOP_SYNC_URL` and `FYNANCE_SHOP_SYNC_TOKEN`; without
them the answer is 400 `sync_not_configured`. The URL must be https (plain http is accepted only for
a loopback host), the date range may not be longer than 36 months (400 `invalid_range`), and a
response larger than 4 MB is refused with a 502.

A dry run shows what would change; the commit then writes every period in one transaction, so a
bad period rolls the whole run back. Each period becomes one record with origin `shop_sync`, keyed
by `start..end` in `external_ref`. A period that crosses the 6 April tax-year boundary is split at
the 6th. Re-running is idempotent: unchanged figures are left alone and changed figures replace
that record's line items in place. Manual and parsed records are never touched. Late refunds and
counts of missing fees or costs are reported in the result but not written.

## Where it shows in the app

Everything is on the **Tax** page (`frontend/src/pages/Tax.tsx`, parts in `pages/tax/`).

| Section | What it shows |
|---|---|
| Header | Taxpayer profile picker, tax year picker with its date window, "Add income source" |
| Readiness checklist | One row per employer and business, plus interest and Gift Aid, showing what is still missing |
| Employment (one card per employer) | Box 1 and 2 figures, the shown-only figures, the records table, the P60 check block with status and "Create year-end adjustment", RSU table with match badges |
| Self-employment (one card per business) | Turnover, expenses, net profit, trading allowance hint, bank expenses with "Map expense categories", annual-accounts check block, "Sync from shop" (only when the source has `sync_kind = shop_app`) |
| Interest and Gift Aid | Taxable and non-taxable interest per account (joint accounts are split by holder), and Gift Aid donations, taken from categorised transactions |
| Dialogs | Source, record, check, expense mapping, shop sync |

**Settings switch**: Settings, Appearance, "Show Tax tab". The Tax tab also appears by itself once
any income record exists (`hooks/use_tax_tab`).

**Import wizard**: the normal Import page handles bank statements, holdings and investments. It has
no payslip or P60 step on master yet. When it is added it will call the parse endpoint with
`return_type.tax_documents` and then the import endpoint above.

## For contributors

| Topic | File |
|---|---|
| Line-type catalogue (box, summed flag, sign, source type) | `backend/src/income/line_types.rs` |
| Summary maths, checks, adjustment lines | `backend/src/income/summary.rs` |
| Record validation, tax year rule | `backend/src/income/validate.rs` |
| RSU vest matching | `backend/src/income/vest.rs` |
| Storage | `backend/src/storage/db/income_store.rs` |
| Routes | `backend/src/server/routes/income.rs`, `income_import.rs`; wired in `server/mod.rs` |
| Document import | `backend/src/importers/tax_documents/` |
| Shop sync (mapping and upsert) | `backend/src/income/sync.rs` |
| Shop sync route | `backend/src/server/routes/income_sync.rs` |
| UI | `frontend/src/pages/Tax.tsx`, `frontend/src/pages/tax/` |

Adding a line type means one new row in the catalogue table and a matching enum variant. The
frontend columns and the summary both read the table, so nothing else needs a list. Money goes through
`Decimal` everywhere in the summary; do not introduce a float. Amounts in other currencies are
converted to the preferred currency at the pay date (employment) or period end, and a missing
exchange rate produces a warning in the summary rather than a failure.

Box numbers come from the 2024-25 SA102 and SA103S forms. Check them against the current year's
form before relying on them for a new tax year.
