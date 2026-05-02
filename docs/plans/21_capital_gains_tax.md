# Plan 21: Capital Gains Tax (CGT) Tracking

**Date:** 2026-04-26
**Status:** RFC — open for review and discussion before implementation begins.
**Target version:** V1

---

## 1. Problem

The current holdings model tracks point-in-time snapshots of share positions. This is sufficient for portfolio valuation but cannot support CGT calculations, which require a full chronological ledger of every acquisition and disposal event.

The goal is to extend fynance so a user can:
- Record every share acquisition (RSU vest, market buy) and disposal (sale, employer withhold)
- View their S104 pool state and unrealised gains at any point in time
- Get a CGT summary for any UK tax year
- Generate a document structured around the HMRC SA108 supplementary pages that can be handed to an accountant or used directly for self-assessment

---

## 2. UK CGT Background

For context, HMRC requires gains to be computed using three matching rules applied in order:

1. **Same-day rule** — match disposal against acquisitions on the same day
2. **30-day rule** — match against acquisitions in the 30 days after the disposal
3. **S104 pool** — remaining shares matched against the running average cost of all other acquisitions

The S104 pool tracks two totals: number of shares and total allowable expenditure (cost). Average cost per share = `total_cost / total_shares`. Each acquisition adds to both; each disposal removes its proportional share.

For RSUs specifically: the vest-date market value is the acquisition cost (HMRC treats the vest as income, so that price becomes the cost basis for CGT). If the employer withholds shares to cover income tax at vest, those are treated as an immediate disposal at vest price.

All amounts must be in GBP. Foreign currency prices need the GBP equivalent at the transaction date.

---

## 3. Proposed Data Model

Three new tables alongside the existing `holdings` table (which is unchanged):

**`holding_events`** — immutable ledger, one row per event (vest, sale, withhold, transfer, split). Never updated or deleted. Each event is tied to an account (same as transactions).

**`s104_pools`** — derived cache of the running pool state per (account, symbol). Recomputed from events whenever new events are added.

**`cgt_disposals`** — one row per disposal after matching rules are applied. Computed automatically when a sale or withhold event is recorded. Stores proceeds, matched cost, gain/loss, and which rule was used.

### ISA handling

The `accounts` table gains an `is_isa` boolean flag. The CGT engine excludes all events tied to ISA accounts from pool calculations and CGT summaries. ISA events are still tracked for record-keeping purposes.

### S104 pool scope

Per (account, symbol) rather than globally across all accounts. This is a simplification — HMRC technically pools across all non-ISA accounts of the same share, but per-account is easier to reason about for now. Open for discussion.

---

## 4. Ingestion

Two paths, consistent with how transactions work today:

1. **External agents** — parse broker statements (Shareworks for RSU vests, T212 activity exports, etc.) and push structured events via `POST /api/holdings/events`. The API is broker-agnostic; agents handle the format-specific parsing.

2. **Manual entry** — the UI allows entering events directly, including backdated historical events. This is the initial path for seeding historical data before CSV importers are built.

The importer architecture should be broker-agnostic so new brokers (Vanguard, Freetrade, etc.) can be added without structural changes.

---

## 5. Backend

A dedicated CGT calculation module (separate from routes) handles:
- Pool maintenance on each acquisition event
- Matching rule application on each disposal event
- Point-in-time queries: replaying events up to a given date to compute pool state and CGT position as it stood then

New API endpoints:
- `POST /api/holdings/events` — record an event
- `GET /api/holdings/events` — list events with filters
- `GET /api/holdings/pools` — current pool state (and at a past date via `?as_at=`)
- `GET /api/cgt/:tax_year` — CGT summary for a tax year (and at a past date via `?as_at=`)
- `GET /api/export?format=cgt&tax_year=` — structured export driving document generation

---

## 6. Frontend

### Stock Transactions view (Portfolio tab)
A dedicated table for holding events — separate from the cash Transactions page. Filterable by account, symbol, event type, date range. Supports manual event entry via a form dialog and CSV import.

### S104 Pool viewer (Portfolio tab)
Read-only table of current pool state per symbol: total shares, total cost, average cost per share, estimated unrealised gain. Includes an "as at" date picker so the user can inspect the pool at any past date.

### CGT Summary (Reports tab)
Tax-year picker showing: total proceeds, total allowable costs, net gain/loss, annual exempt amount, taxable gain. Includes a full disposals table with matching rule detail. Also supports an "as at" date for mid-year planning.

### HMRC Document Generation
A print/export-ready document generated from the CGT summary. Structured around the SA108 supplementary pages:
- Taxpayer details (name, UTR — stored in profile)
- Tax year and disposal summary totals
- Full disposal schedule
- S104 pool workings per symbol (the supporting evidence HMRC can request)

Exported as PDF (browser-side) or CSV. No server-side document generation.

---

## 7. Open Questions

These need input before implementation begins:

1. **S104 pool scope** — should the pool be per (account, symbol) as proposed, or globally across all non-ISA accounts holding the same symbol? The latter is more HMRC-correct but significantly more complex.

2. **Shareworks CSV format** — a sample export is needed to confirm field names and structure before building the importer.

3. **T212 activity export** — T212 offers multiple export types. Which contains the per-share acquisition/disposal detail needed for CGT?

4. **Stock splits** — if any held shares have undergone a split, the pool quantity needs retroactive adjustment. Should the app handle this at launch or defer it?

5. **UTR field on profile** — the HMRC document needs the user's Unique Taxpayer Reference. Should this be added to the profile model, or is it out of scope and left to the user to fill in manually on the generated document?

6. **Annual exempt amount** — currently £3,000 (2024-25 onwards). Should this be hardcoded per tax year in the codebase, or user-configurable in case of changes?
