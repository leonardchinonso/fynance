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

For RSUs specifically: the vest-date market value is the acquisition cost (HMRC treats the vest as income, so that price becomes the cost basis for CGT). If the employer withholds shares to cover income tax at vest, those are treated as an immediate disposal at vest price (`withhold` event). Stock splits (`split` event) adjust the pool quantity retroactively — the total cost stays the same, only the share count and price per share change.

HMRC requires all CGT amounts in GBP. The `investments` table stores prices in their native currency; the GBP equivalent is computed on the fly using historic FX rates at the event date. HMRC requires each leg to be converted separately — disposal proceeds use the disposal-date rate, acquisition costs use the acquisition-date rate. You cannot convert the gain itself. HMRC publishes monthly exchange rates; the actual transaction rate is also acceptable.

Historic rate lookup for CGT is a separate concern from the `currencies` table, which only stores the current preferred display rate. The CGT calculation flow will look up rates from a dedicated historic rates store (date-keyed), independent of the portfolio FX display logic.

---

## 3. Proposed Data Model

One new table alongside the existing `holdings` table (which is unchanged):

**`investments`** — append-only ledger in intent, one row per investment event (vest, buy, sell, transfer, withhold, split). Each event is tied to an `account_id` (not a holding — holdings are point-in-time snapshots, not a suitable parent for individual events). The symbol is carried on each row directly. Events can be edited or deleted to correct mistakes — since pool state and CGT disposals are computed on the fly, any correction is automatically reflected everywhere with no cache to invalidate.

```sql
CREATE TABLE IF NOT EXISTS investments (
    id               TEXT PRIMARY KEY,       -- UUID
    account_id       TEXT NOT NULL,          -- FK to accounts
    event_type       TEXT NOT NULL,          -- 'vest' | 'buy' | 'sell' | 'transfer' | 'withhold' | 'split'
    symbol           TEXT NOT NULL,          -- ticker or ISIN (e.g. 'AAPL', 'VWRL'); name derived from holdings at query time
    date             TEXT NOT NULL,          -- ISO 8601 datetime (YYYY-MM-DDTHH:MM:SS); date-only imports use T00:00:00
    quantity         TEXT NOT NULL,          -- Decimal as TEXT (shares/units)
    price_per_share  TEXT NOT NULL,          -- Decimal as TEXT, in native currency
    fee              TEXT,                   -- Decimal as TEXT; broker commission + stamp duty; assumed same currency as the instrument; NULL for splits/transfers
    currency         TEXT NOT NULL,          -- native currency of price and fee (e.g. 'USD', 'GBP')
    notes            TEXT,                   -- optional user annotation
    fingerprint      TEXT NOT NULL UNIQUE,   -- SHA-256 for dedup, consistent with transactions
    created_at       TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    FOREIGN KEY (account_id) REFERENCES accounts(id)
);

CREATE INDEX IF NOT EXISTS idx_investments_account ON investments(account_id);
CREATE INDEX IF NOT EXISTS idx_investments_symbol  ON investments(symbol);
CREATE INDEX IF NOT EXISTS idx_investments_date    ON investments(date);
```

S104 pool state and CGT disposals are computed on the fly from `investments` rather than cached in separate tables. At personal-finance scale (hundreds to low thousands of events), SQLite can replay and aggregate all events in under 1ms — a derived-table cache would add invalidation complexity with no measurable benefit. If query performance becomes a problem at much larger event volumes, `s104_pools` and `cgt_disposals` cache tables can be introduced at that point.

### ISA handling

`accounts.type` gains a new value: `investment_isa`. ISA status is a property of the account, not individual events — every event in an `investment_isa` account is automatically sheltered. The CGT engine excludes all events where the joined account has `type = 'investment_isa'` from pool calculations and CGT summaries. Events are still stored for record-keeping.

This is consistent with how `pension` works — it is its own account type rather than a flag on top of `investment`. A regular GIA broker account uses `type = 'investment'`; an ISA wrapper uses `type = 'investment_isa'`.

### S104 pool scope

HMRC requires the pool to be per (person, symbol) across all non-ISA accounts — not per account. Holding 50 AAPL in a Trading 212 GIA and 20 AAPL in a Freetrade GIA means a single S104 pool of 70 AAPL. The per-account simplification would produce incorrect CGT figures and is not acceptable. Implementation must pool globally across all non-`investment_isa` accounts for the same symbol.

---

## 4. Ingestion

Two paths, consistent with how transactions work today:

1. **External agents** — parse broker statements (Shareworks for RSU vests, T212 activity exports, etc.) and push structured events via `POST /api/capital/events`. The API is broker-agnostic; agents handle the format-specific parsing.

2. **Manual entry** — the UI allows entering events directly, including backdated historical events. This is the initial path for seeding historical data before CSV importers are built.

The importer architecture should be broker-agnostic so new brokers (Vanguard, Freetrade, etc.) can be added without structural changes.

---

## 5. Backend

A dedicated CGT calculation module (separate from routes) handles:
- S104 pool state: computed by replaying all acquisition events up to a given date (`SELECT SUM` over `investments`)
- CGT disposals: computed by replaying all events in order and applying matching rules — no pre-stored results
- Point-in-time queries: filtering events by date gives pool state and CGT position as it stood at any past date

New API endpoints:
- `POST /api/investments` — record an investment event
- `GET /api/investments` — list events with filters (account, symbol, event_type, date range)
- `PATCH /api/investments/:id` — correct a mistaken event
- `DELETE /api/investments/:id` — remove a mistaken event
- `GET /api/investments/pools` — current S104 pool state (and at a past date via `?as_at=`)
- `GET /api/cgt/:tax_year` — CGT summary for a tax year (and at a past date via `?as_at=`)
- `GET /api/export?format=cgt&tax_year=` — structured export driving document generation

---

## 6. Frontend

### Investments view (Portfolio tab)
A dedicated table for investment events — separate from the cash Transactions page. Filterable by account, symbol, event type, date range. Supports manual event entry via a form dialog and CSV import.

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

1. **Transfer disposal treatment** — HMRC treats a transfer between your own separate brokerage accounts as a disposal (unless it is a nominee-to-beneficial-owner transfer, which is not). A `transfer` event therefore needs to be recorded as both a disposal (out of the source account) and an acquisition (into the destination account). Should this be two separate rows in `investments`, or one row with a `destination_account_id` field? Two rows is simpler for the CGT engine; one row makes the transfer intent explicit.

2. **Shareworks CSV format** — a sample export is needed to confirm field names and structure before building the importer.

3. **T212 activity export** — T212 offers multiple export types. Which contains the per-share acquisition/disposal detail needed for CGT?

4. **UTR field on profile** — the HMRC document needs the user's Unique Taxpayer Reference. Should this be added to the profile model, or is it out of scope and left to the user to fill in manually on the generated document?

5. **Annual exempt amount** — currently £3,000 from 2024-25 onwards (reduced from £6,000 in 2023-24 and £12,300 in 2022-23). Should past-year amounts be hardcoded per tax year in the codebase, or user-configurable?
