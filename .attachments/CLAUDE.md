# Attachments — Agent Context

This folder is the user's local workspace for raw financial source documents (bank statements, broker statements, equity-comp statements, etc.) that get imported into fynance. The contents are **gitignored** and never committed; only this guidance file is tracked.

## CRITICAL RULES

- **Never use `POST /api/import/csv` or the bulk CSV upload endpoints.** Always import transactions via `POST /api/import` with structured JSON. Do not attempt inline curl heredocs (quoting breaks on merchant names with apostrophes or special characters).
- **Two-phase import workflow — never hit the API directly from a generation script:**
  1. **Generate phase:** Write a Python script that reads source documents and outputs a single JSON file (e.g. `payloads_<period>.json`) containing all API calls to be made — transactions, investment events, and holdings snapshots — structured as a list of `{"endpoint": "/api/...", "payload": {...}}` objects. The script must NOT call `requests` or hit any API.
  2. **Review phase:** The user reviews the generated JSON file before anything is posted.
  3. **Import phase:** Only after user approval, run a separate short script (or the same script with a `--post` flag) that reads the JSON file and POSTs each payload to the API. This is the only script that is allowed to call `requests`.
- **Closing snapshots are dedicated zero-value rows.** When a holding/pot/sub-account is closed, the closing snapshot must set `value="0"`, `quantity="0"`, `price_per_unit="0"`, and `is_closed=true`, dated at the actual close date. Do **not** carry over the last positive balance into the closed row — closing implies the funds were transferred out, so the closing snapshot represents the post-close zero state. The last positive snapshot before the close should be a separate, normal `is_closed=false` row. This applies to every source: bank pots, fund switches, anything that ends.
- **USD-denominated stock trades almost always charge a fee** (FX/exchange/government). When importing, a zero fee on a USD-denominated trade is almost certainly a parse mistake — re-check the executed-trades table for the FX/exchange/government fee column. GBP-denominated trades (e.g. LSE-listed ETFs) often have no fee; that's fine. Use `null` only if the statement genuinely shows no fee.

---

## Folder layout (suggestion, not required)

The user's workspace is gitignored, so each user organises it however they like. A common shape:

```
.attachments/
├── <broker_or_bank>_<period>/
│   ├── Monthly-Statement-YYYY-MM.pdf
│   └── Transactions-Statement-YYYY-MM-DD-YYYY-MM-DD.pdf
├── <other_source>_<period>/
└── CLAUDE.md   # this file
```

Treat the layout you find as authoritative for the current session — don't assume any particular subfolder name. Ask the user where the documents live if it isn't obvious.

## Accounts in scope

Each source corresponds to one or more fynance `account_id`s. **Do not assume any specific account IDs exist.** Ask the user (or check `GET /api/holdings/accounts` / `GET /api/transactions/accounts`) to discover what IDs to use, then refer to them by placeholder in any plan you write up:

| Placeholder              | Typical source                                      |
|--------------------------|-----------------------------------------------------|
| `<broker_isa_id>`        | Stocks ISA at a retail broker                       |
| `<broker_invest_id>`     | General investment account at the same broker       |
| `<bank_account_id>`      | Current account + pots/jars at a retail bank        |
| `<rsu_account_id>`       | Equity-compensation account (Shareworks, etc.)      |

Substitute the real IDs when running scripts; keep examples in this file abstract.

---

# Generic Methodology — Retail Broker Statements (e.g. Trading 212, Vanguard, Fidelity)

## Step 1 — Read both source documents

Always use both together where available. **Read PDFs directly** — do not use exported PNG folders. A single Read call on a PDF returns all pages as structured text, which is faster, cheaper, and more accurate than reading individual PNG pages.

1. **Monthly Statement PDF** — one Read call gets the whole document. Typical sections:
   - Overview (account values, deposits, withdrawals for the month)
   - Per-sub-account: executed trades, open positions summary (end-of-month quantities and prices), transactions and dividends, cash breakdown
   - Glossary / disclosures (ignore)

   Page count and order vary between brokers and months — confirm from headings rather than page numbers.

2. **Transactions Statement PDF** (if available) — authoritative cash ledger showing every deposit, transfer, and interest payment with exact UTC timestamps. Use it to confirm money flow dates. Skip pages that contain only daily interest entries.

## Step 2 — Post cash flow transactions

Use `POST /api/import` to record deposits and transfers as transactions. These feed the cash flow view and ensure money-in is visible in the budget/reports.

```json
{
  "account_id": "<broker_invest_id>",
  "transactions": [
    {
      "date": "YYYY-MM-DDTHH:MM:SS",
      "description": "Standing deposit from external bank",
      "amount": "100.00",
      "currency": "GBP",
      "category": "Transfers",
      "notes": "External bank deposit"
    }
  ]
}
```

Rules:
- Source: the transactions and dividends page of the monthly statement, and the transactions statement
- `amount`: positive for money in, negative for money out (same convention as the rest of fynance)
- Record external standing deposits, and any inter-account transfers
- **Do not record** daily interest-on-cash entries — too granular, low value
- Internal broker transfers (Invest → ISA) appear as negative on one side and positive on the other; record both sides so net cash is correct

## Step 3 — Post investment events

Use `POST /api/investments` for each executed trade. **Only post trades, not cash movements or interest.**

### investments table schema

The `investments` table is an **immutable event ledger** — one row per share acquisition or disposal event. It is the source of truth for CGT calculations (S104 pool computed on the fly from this table). Never update or delete rows.

| Field | Type | Notes |
|---|---|---|
| `account_id` | TEXT | fynance account ID |
| `event_type` | TEXT | `vest` \| `buy` \| `sell` \| `transfer` \| `withhold` \| `split` |
| `symbol` | TEXT | Ticker or ISIN |
| `date` | TEXT | ISO 8601 datetime (`YYYY-MM-DDTHH:MM:SS`) |
| `quantity` | TEXT | Decimal as string (shares/units) |
| `price_per_share` | TEXT | Decimal as string, in native currency |
| `fee` | TEXT\|null | Broker commission + stamp duty; same currency as instrument; `null` for splits/transfers |
| `currency` | TEXT | Native currency of price and fee (`USD`, `GBP`, `GBX`) |
| `notes` | TEXT\|null | Free text |
| `fingerprint` | TEXT | Unique dedup key — safe to re-post the same event |

**Event type semantics:**
- `vest` — shares granted/released to you (RSU vest + release, or SAR share delivery)
- `buy` — open-market purchase
- `sell` — open-market disposal for cash
- `withhold` — shares withheld/sold to cover tax at vest ("sell to cover" portion); treated as a disposal at cost for CGT but not a user-initiated sale
- `transfer` — shares moved between accounts or brokers (not a disposal)
- `split` — stock split adjustment

```json
{
  "account_id": "<broker_isa_id>",
  "event_type": "buy",
  "symbol": "EXAMPLE_TICKER",
  "date": "YYYY-MM-DDTHH:MM:SS",
  "quantity": "0.50000000",
  "price_per_share": "100.00",
  "fee": "0.05",
  "currency": "USD",
  "notes": "Monthly DCA buy"
}
```

Rules:
- `event_type`: `buy` or `sell` for retail-broker trades (RSU events are covered separately below)
- `date`: use the exact UTC execution timestamp from the executed trades table
- `price_per_share`: the execution price from the statement, in the instrument's native currency
- `currency`: instrument currency — `USD` for US stocks, `GBX` for LSE-listed ETFs, etc.
- `fee`: exchange and government fees column. For USD-denominated trades, a fee is almost always present — if you've parsed `"0"`, double-check the statement. For GBP-denominated trades a blank/zero fee is normal. Use `null` only when the statement genuinely shows no fee.
- `notes`: include enough context to identify the trade later (e.g. `"<Month> monthly buy"`)
- The API deduplicates by fingerprint so re-posting the same trade is safe

**Do not post:** interest on cash, dividends, deposits, transfers between accounts.

## Step 4 — Post end-of-month holdings snapshots

Use `POST /api/holdings/import` with all positions for one account at once. Use the **open positions summary** page for quantities and prices.

```json
{
  "account_id": "<broker_isa_id>",
  "holdings": [
    {
      "account_id": "<broker_isa_id>",
      "symbol": "EXAMPLE_ETF",
      "name": "Example S&P 500 ETF (Acc)",
      "holding_type": "etf",
      "quantity": "10.00000000",
      "price_per_unit": "500.00",
      "value": "5000.00",
      "currency": "GBP",
      "as_of": "YYYY-MM-DDT23:59:59",
      "is_closed": false
    }
  ]
}
```

Rules:
- `as_of`: last day of the month at `23:59:59` — e.g. `YYYY-04-30T23:59:59`, `YYYY-03-31T23:59:59`
- `price_per_unit`: end-of-month price from the open positions summary (often labelled "Price")
- `value`: the VALUE column from the open positions summary (quantity × price in the displayed currency)
- `currency`: match the displayed VALUE column currency. LSE-listed ETFs are commonly quoted in GBX (pence) but VALUE is shown in GBP — store `currency` as `GBP` in that case.
- `holding_type`: `etf` for ETFs, `stock` for individual equities, `cash` for cash holdings
- Post each sub-account separately (one call per `account_id`)
- This endpoint upserts — safe to re-run

**Also include a `_CASH` holding** for each account to capture the total cash balance (which implicitly includes accumulated dividends and interest):

```json
{
  "account_id": "<broker_isa_id>",
  "symbol": "_CASH",
  "name": "Cash",
  "holding_type": "cash",
  "quantity": "1",
  "price_per_unit": "100.00",
  "value": "100.00",
  "currency": "GBP",
  "as_of": "YYYY-MM-DDT23:59:59",
  "is_closed": false
}
```

The cash value comes from the **cash breakdown page** of the monthly statement (Total cash figure). This is where dividends and interest end up — by snapshotting the total cash you implicitly capture them without needing to record each dividend individually.

## Step 5 — Verify

```bash
curl "http://localhost:7433/api/investments?account_id=<broker_isa_id>"
curl "http://localhost:7433/api/holdings?account_ids=<broker_isa_id>,<broker_invest_id>"
```

Check that:
- Investment event count matches the number of rows in the executed trades table
- Holdings snapshot `as_of` date matches the month end
- No duplicate entries (fingerprint dedup means re-runs are safe but worth confirming)

---

## Patterns worth watching for

- **Standing deposits:** many brokers receive a recurring deposit on a fixed day of the month. If one stops or is redirected, the cash will instead show as a transfer to another sub-account.
- **Batched buys:** retail DCA accounts often execute many positions in one session on or near deposit day. Don't skip any — the executed-trades table is the source of truth.
- **GBX vs GBP for LSE ETFs:** prices listed in GBX (pence), but the VALUE column is already in GBP. Store `currency` as `GBP` and use the GBP value directly.
- **FX rate:** the open positions summary often shows the end-of-month Bloomberg GBP/USD rate. Useful for cross-checking USD-denominated holdings, not needed for storage.
- **Dividends:** dividends are paid into the account cash balance, so they are implicitly captured by the `_CASH` holding snapshot. No separate dividend event is needed unless the user explicitly wants a per-dividend ledger.

## Month processing order

When backfilling many months, work backwards (most recent → oldest). Each month's statement is self-contained: the open positions summary always shows the cumulative quantities at month end, so you do not need to reconstruct positions from scratch each time.

## Multi-month imports — use subagents

When processing more than one month in a single session, **spawn a separate subagent per month**. Even though PDFs are much more efficient than PNGs, loading several months of PDFs into one conversation still accumulates context. A subagent per month keeps each context clean and focused.

Rules:
- One subagent = one month. Pass it the month name, the path to that month's PDF, the transactions statement PDF, and this CLAUDE.md as context.
- Run subagents **sequentially**, not in parallel — each month's verify step should pass before the next starts.
- For a single month you do not need a subagent; load directly in the main conversation.
- The API's fingerprint dedup means a subagent re-running a month is safe, but check `rows_inserted` / investment count after each month to confirm clean inserts.

### Which parts of the transactions statement each subagent needs

The transactions statement may cover all months in one PDF. Each subagent should read the full PDF but focus only on entries dated within the target month — skip daily interest entries, which have no import value per the methodology. The subagent should extract deposits, transfers, and any other non-interest cash movements for its target month only.

---

# Generic Methodology — Bank Statements with Pots/Jars (e.g. Monzo, Starling)

## Source documents

Bank export bundles typically contain:
- **Personal Account statement** (PDF, sometimes also exported as PNGs) — reverse-chronological transaction list with a running Balance column showing the personal account balance (excluding pots) after each transaction
- **Pot/jar statements** (PDF, one per pot) — each shows pot name, type, total balance at statement end, and a reverse-chronological list of deposits/withdrawals with running balance

The statement cover page often shows three key totals: **Total balance** (personal + all pots), **Personal Account balance** (excluding pots), and **Balance in Pots**.

## Step 1 — Read all source pages

Read all source pages in parallel. The personal account statement pages run in reverse date order. Pot statement pages follow after. Identify:
- All pot names and types
- Whether each pot is still open or was closed (closed pots typically show a "This Pot was closed on DD/MM/YYYY" notice)

## Step 2 — Derive end-of-month balances per pot

The balance column shows balance **after** each transaction. To find the end-of-month balance for a given pot:
- Find the last transaction on or before the last day of the month
- Its balance column value is the end-of-month balance for that pot

For the personal account, do the same from the personal statement pages.

End-of-month **total** = personal balance + sum of all pot balances at that date.

Verify against the cover page total — these must match.

## Step 3 — Post transactions

Write all transactions to a Python script file first (do not attempt inline curl heredocs — quoting breaks with merchant names containing apostrophes or special characters).

Use `POST /api/import`:

```json
{
  "account_id": "<bank_account_id>",
  "transactions": [
    {
      "date": "YYYY-MM-DDTHH:MM:SS",
      "description": "EXAMPLE MERCHANT",
      "amount": "10.00",
      "currency": "GBP",
      "category_id": "<uuid>",
      "notes": "Optional context"
    }
  ]
}
```

Rules:
- `amount`: positive = money in, negative = money out
- `category_id`: must be a valid UUID from the categories table — resolve via `GET /api/transactions/categories` first
- Include pot transfers (both directions) for full transparency
- Include joint account transfers (both inbound and outbound)
- Include zero-amount transactions (e.g. foreign card checks)
- Round-up transfers to savings pots are their own transactions — include them
- The API deduplicates by fingerprint (sha256 of date + amount + account_id) — safe to re-run
- Response shape: `{"rows_total": N, "rows_inserted": N, "rows_duplicate": N, ...}` — check `rows_inserted` not `inserted`

## Step 4 — Post per-pot holdings snapshots

Use `POST /api/holdings/import`. Post each pot as a separate symbol so history is tracked independently.

Symbol naming convention:
- `_CASH_MAIN` — personal current account
- `_CASH_<SHORTNAME>` — one per pot, where `<SHORTNAME>` is a short, stable identifier for the pot (e.g. `_CASH_EMERGENCY`, `_CASH_HOLIDAY`)

```json
{
  "account_id": "<bank_account_id>",
  "holdings": [
    {
      "account_id": "<bank_account_id>",
      "symbol": "_CASH_MAIN",
      "name": "Current Account",
      "holding_type": "cash",
      "quantity": "1",
      "price_per_unit": null,
      "value": "1000.00",
      "currency": "GBP",
      "as_of": "YYYY-MM-DDT00:00:00",
      "is_closed": false
    }
  ]
}
```

Rules:
- Post one snapshot per pot per month-end (use `T00:00:00` for `as_of`)
- For a pot closed mid-month, post its final snapshot with `"value": "0.00"` and `"is_closed": true` using the actual close date as `as_of`
- Historical snapshots that predate a pot's closure should have `"is_closed": false` — only the final zero-balance snapshot is closed
- After importing, you can also PATCH a closed pot via `PATCH /api/holdings/<account_id>/<symbol>` with `{"is_closed": true, "as_of": "YYYY-MM-DD"}` if you need to flip an existing snapshot
- This endpoint upserts — safe to re-run

## Step 5 — Verify

```bash
curl "http://127.0.0.1:7433/api/holdings?account_id=<bank_account_id>"
```

Expected: only open pots appear, each at their latest snapshot. Closed pots are excluded. If a pot appears that should be closed, check that its latest snapshot (by `as_of`) has `is_closed = true`.

---

# Generic Methodology — Equity Compensation Statements (e.g. Shareworks / Morgan Stanley)

## Source documents

A typical RSU/PRSU/SAR account statement is a single comprehensive PDF covering the entire grant history. **Read it with a single PDF Read call** — do not attempt page-by-page reads.

## What an equity-comp statement contains

- **Account summary page**: total value, breakdown by instrument type (P-RSU, RSU, common stock), future vesting schedule by year
- **Per-instrument summary tables and full activity log**: every grant, vest, release event
- **Detailed release breakdowns**: each release shows price, quantity, gross value, tax withheld, shares disbursed
- **SAR section** (if applicable): summary and exercise history with strike price and quantity
- **Stock holdings ledger**: full share movement history (releases, sales, transfers out)
- **Ad-hoc withdrawal/sale events**: each with gross proceeds, fees, net proceeds

## Event mapping

Each statement event maps to an `investments` table event type as follows:

| Statement event | investments `event_type` | Notes |
|---|---|---|
| RSU/P-RSU Vest | (no entry needed alone — vest is confirmed by the Release) | |
| RSU/P-RSU Release — shares disbursed | `vest` | quantity = "Number of Restricted Awards Disbursed"; price = "Release Price" |
| RSU/P-RSU Release — shares sold to cover tax | `withhold` | quantity = "Number of Restricted Awards Sold"; price = "Sale Price" |
| SAR Exercise — shares delivered | `vest` | quantity = appreciation shares delivered after tax withhold |
| SAR Exercise — shares withheld for tax | `withhold` | quantity = shares withheld for taxes |
| Ad-hoc sale (Withdrawal) | `sell` | quantity = shares sold; price = market price per unit from statement |
| Transfer out | `transfer` | quantity = shares transferred; price = share price on that date |

**Do not post** the vest event separately from the release — the release is the taxable event. One release = one `vest` row + one `withhold` row.

## Step 1 — Read the statement

Single Read call on the statement PDF. Extract for each release event:
- Grant name, release date, settlement date
- Number Released, Number Sold (withheld for tax), Number Disbursed
- Release Price (used as `price_per_share` for the `vest` row)
- Sale Price (used as `price_per_share` for the `withhold` row)
- Gross Release Value, International Tax Withholding amount

## Step 2 — Post investment events

Use `POST /api/investments` for each event. Post them in chronological date order.

**RSU/P-RSU release — vest portion (shares actually received):**
```json
{
  "account_id": "<rsu_account_id>",
  "event_type": "vest",
  "symbol": "EMPLOYER_TICKER",
  "date": "YYYY-MM-DDT00:00:00",
  "quantity": "100",
  "price_per_share": "20.00",
  "fee": null,
  "currency": "USD",
  "notes": "MM/DD/YYYY - RSU release <reference>"
}
```

**RSU/P-RSU release — withhold portion (sold to cover tax):**
```json
{
  "account_id": "<rsu_account_id>",
  "event_type": "withhold",
  "symbol": "EMPLOYER_TICKER",
  "date": "YYYY-MM-DDT00:00:00",
  "quantity": "100",
  "price_per_share": "20.00",
  "fee": null,
  "currency": "USD",
  "notes": "MM/DD/YYYY - RSU release <reference> — sell to cover tax"
}
```

**Ad-hoc sale (Withdrawal):**
```json
{
  "account_id": "<rsu_account_id>",
  "event_type": "sell",
  "symbol": "EMPLOYER_TICKER",
  "date": "YYYY-MM-DDT00:00:00",
  "quantity": "10",
  "price_per_share": "25.00",
  "fee": "0.03",
  "currency": "USD",
  "notes": "Ad-hoc withdrawal <reference>"
}
```

Rules:
- `date`: use the Release Date (not Settlement Date) for vest/withhold; use the withdrawal date for sells
- `price_per_share`: "Release Price" for vest rows, "Sale Price" for withhold rows, "Market Price Per Unit" for sell rows
- `fee`: use the "Supplemental Transaction Fee" from the sale breakdown; `null` for vest/withhold rows
- `notes`: always include the grant name or reference number for traceability
- Symbol is the employer's ticker; ask the user if it isn't obvious from the statement
- Currency is typically `USD` for US-listed equity comp
- The API deduplicates by fingerprint — safe to re-run

## Step 3 — Post holdings snapshot

After posting all events, post a current holdings snapshot reflecting the shares the user holds today.

From the statement summary page, take:
- **Available Quantity** under the common-stock section — this is the snapshot quantity
- **Current price** as printed on the statement (or the statement date's closing price)
- Do **not** include unvested RSU/P-RSU grants in the holdings snapshot — they are not owned yet

```json
{
  "account_id": "<rsu_account_id>",
  "holdings": [
    {
      "account_id": "<rsu_account_id>",
      "symbol": "EMPLOYER_TICKER",
      "name": "<Employer name> Inc.",
      "holding_type": "stock",
      "quantity": "100",
      "price_per_unit": "100.00",
      "value": "10000.00",
      "currency": "USD",
      "as_of": "YYYY-MM-DDT00:00:00",
      "is_closed": false
    }
  ]
}
```

Notes:
- `quantity`: from the "Available Quantity" row for common stock on the summary page
- `as_of`: use the statement date (`YYYY-MM-DDT00:00:00`)
- Unvested RSU/P-RSU grants should NOT appear as holdings — they are reflected in `future_value` in the portfolio summary but are not yet owned
- Update this snapshot each time a new statement is generated

## Step 4 — Verify

```bash
curl "http://localhost:7433/api/investments?account_id=<rsu_account_id>"
curl "http://localhost:7433/api/holdings?account_id=<rsu_account_id>"
```

Expected: investment event count should match total number of release events × 2 (one vest + one withhold per release) plus ad-hoc sales. Holdings snapshot should show current quantity at current price.
