# Attachments — Agent Context

This folder contains financial statements for importing historical investment data into fynance.

## Folder structure

```
.attachments/
├── Trading 212  Jan to april/
│   ├── Monthly-Statement-2026-MM.pdf        # one per month
│   ├── Monthly-Statement-2026-MM/           # PDF pages exported as PNGs (01, 02, ...)
│   └── Transactions-Statement-2026-01-01-2026-05-05/  # running cash ledger, all accounts
├── Monzo 2026 Jan to april/                 # bank statement (separate workflow)
└── CLAUDE.md                                # this file
```

## Accounts in scope

| fynance account_id  | T212 account name     | T212 account ID | Currency |
|---------------------|-----------------------|-----------------|----------|
| `ope-t212-isa`      | Stocks ISA            | 23158320        | GBP      |
| `ope-t212-invest`   | Invest                | 30566960        | GBP      |

The Cash ISA (T212 ID 32918413) has had zero activity and can be ignored.

---

## Methodology

### Step 1 — Read both source documents

Always use both together. **Read the PDFs directly** — do not use the PNG folders. A single Read call on a PDF returns all pages as structured text, which is faster, cheaper, and more accurate than reading individual PNGs.

1. **Monthly Statement PDF** (`Monthly-Statement-2026-MM.pdf`) — one Read call gets the whole document. Key pages:
   - Page 1: Overview (account values, deposits, withdrawals for the month)
   - Page 2: Invest account — executed trades
   - Page 3: Invest account — open positions summary (end-of-month quantities and prices)
   - Page 4: Invest account — transactions and dividends
   - Page 5: Stocks ISA — executed trades
   - Page 6: Stocks ISA — open positions summary
   - Page 7: Stocks ISA — cash breakdown
   - Page 8: Stocks ISA — transactions and dividends
   - Remaining pages: Cash ISA (ignore if empty), Glossary, Disclosures (ignore)

   Note: page count and order may vary slightly between months — confirm from headings.

2. **Transactions Statement PDF** — one Read call gets the whole document. This is the authoritative cash ledger showing every deposit, transfer, and interest payment with exact UTC timestamps. Use it to confirm money flow dates. Skip pages that contain only daily interest entries.

### Step 2 — Post cash flow transactions

Use `POST /api/import` to record deposits and transfers as transactions. These feed the cash flow view and ensure money-in is visible in the budget/reports.

```json
{
  "account_id": "ope-t212-invest",
  "transactions": [
    {
      "date": "2026-01-26T02:06:19",
      "description": "T212 Invest deposit",
      "amount": "500",
      "currency": "GBP",
      "category": "Transfers",
      "notes": "JP Morgan bank deposit"
    }
  ]
}
```

Rules:
- Source: the transactions and dividends page of the monthly statement, and the transactions statement
- `amount`: positive for money in, negative for money out (same convention as the rest of fynance)
- Record JP Morgan standing deposits, and any inter-account transfers
- **Do not record** daily interest-on-cash entries — too granular, low value
- Internal T212 transfers (Invest → ISA) appear as `-500` on Invest and `+500` on ISA; record both sides so net cash is correct

### Step 3 — Post investment events

Use `POST /api/investments` for each executed trade. **Only post trades, not cash movements or interest.**

```json
{
  "account_id": "ope-t212-isa",
  "event_type": "buy",
  "symbol": "NVDA",
  "date": "2026-04-29T13:30:00",
  "quantity": "0.39159565",
  "price_per_share": "212.67",
  "fee": "0.09",
  "currency": "USD",
  "notes": "April monthly buy"
}
```

Rules:
- `event_type`: `buy` or `sell` (most months will be buys only)
- `date`: use the exact UTC execution timestamp from the executed trades table
- `price_per_share`: the execution price from the statement, in the instrument's native currency
- `currency`: instrument currency — `USD` for US stocks, `GBX` for LSE-listed ETFs (CSP1, FWRG)
- `fee`: exchange and government fees column; use `"0"` if blank
- `notes`: `"<Month> monthly buy"` e.g. `"March monthly buy"`
- The API deduplicates by fingerprint so re-posting the same trade is safe

**Do not post:** interest on cash, dividends, deposits, transfers between accounts.

### Step 4 — Post end-of-month holdings snapshots

Use `POST /api/holdings/import` with all positions for one account at once. Use the **open positions summary** page for quantities and prices.

```json
{
  "account_id": "ope-t212-isa",
  "holdings": [
    {
      "account_id": "ope-t212-isa",
      "symbol": "CSP1",
      "name": "iShares Core S&P 500 (Acc)",
      "holding_type": "etf",
      "quantity": "62.08476055",
      "price_per_unit": "566.89",
      "value": "35195.23",
      "currency": "GBP",
      "as_of": "2026-04-30T23:59:59",
      "is_closed": false
    }
  ]
}
```

Rules:
- `as_of`: last day of the month at `23:59:59` — e.g. `2026-04-30T23:59:59`, `2026-03-31T23:59:59`
- `price_per_unit`: end-of-month Bloomberg price from the open positions summary (labelled "Price")
- `value`: the VALUE column from the open positions summary (quantity × price in native currency)
- `currency`: `GBP` for CSP1 and FWRG (GBX prices but values shown in GBP); `USD` for all US stocks
- `holding_type`: `etf` for CSP1/FWRG, `stock` for everything else
- Post both accounts separately in two calls
- This endpoint upserts — safe to re-run

**Also include a `_CASH` holding** for each account to capture the total cash balance (which implicitly includes accumulated dividends and interest):

```json
{
  "account_id": "ope-t212-isa",
  "symbol": "_CASH",
  "name": "Cash",
  "holding_type": "cash",
  "quantity": "1",
  "price_per_unit": "641.13",
  "value": "641.13",
  "currency": "GBP",
  "as_of": "2026-04-30T23:59:59",
  "is_closed": false
}
```

The cash value comes from the **cash breakdown page** of the monthly statement (Total cash figure). This is where dividends and interest end up — by snapshotting the total cash you implicitly capture them without needing to record each dividend individually.

### Step 5 — Verify

```bash
curl "http://localhost:7433/api/investments?account_id=ope-t212-isa"
curl "http://localhost:7433/api/holdings?account_ids=ope-t212-isa,ope-t212-invest"
```

Check that:
- Investment event count matches the number of rows in the executed trades table
- Holdings snapshot `as_of` date matches the month end
- No duplicate entries (fingerprint dedup means re-runs are safe but worth confirming)

---

## Recurring patterns observed

- **Invest account**: receives a £500 JP Morgan standing deposit on the 26th of each month (Jan, Feb, Mar). In April this was redirected — £500 transferred out to the ISA instead. No trades executed directly in Invest (all cash sits there or is transferred).
- **ISA account**: receives deposits then executes a batch of market buys (typically 11 positions) in one session, usually on or around the 29th of the month.
- **ETF prices**: CSP1 and FWRG are listed in GBX (pence) on the statement but the VALUE column is already in GBP. Store `currency` as `GBP` and use the GBP value directly.
- **FX rate**: the open positions summary shows the end-of-month Bloomberg GBP/USD rate in the FX RATE column. For April it was 1.36078. Not needed for storage but useful for cross-checking.
- **Dividends**: NVDA pays a small quarterly dividend (£0.05–0.10 net after 15% WHT). Dividends are paid into the account cash balance, so they are implicitly captured by the `_CASH` holding snapshot. No separate dividend event is needed.

## Month processing order

Work backwards from April: April → March → February → January.
Each month's statement is self-contained. The open positions summary always shows the cumulative quantities at month end, so you do not need to reconstruct positions from scratch each time.

## Multi-month imports — use subagents

When processing more than one month in a single session, **spawn a separate subagent per month**. Even though PDFs are much more efficient than PNGs, loading several months of PDFs into one conversation still accumulates context. A subagent per month keeps each context clean and focused.

Rules:
- One subagent = one month. Pass it the month name, the path to that month's PDF, the transactions statement PDF, and this CLAUDE.md as context.
- Run subagents **sequentially**, not in parallel — each month's verify step should pass before the next starts.
- For a single month you do not need a subagent; load directly in the main conversation.
- The API's fingerprint dedup means a subagent re-running a month is safe, but check `rows_inserted` / investment count after each month to confirm clean inserts.

### Which parts of the transactions statement each subagent needs

The transactions statement covers all months in one PDF. Each subagent should read the full PDF but focus only on entries dated within the target month — skip daily interest entries, which have no import value per the methodology. The subagent should extract deposits, transfers, and any other non-interest cash movements for its target month only.

---

# Monzo Bank — Import Methodology

## Source documents

The Monzo export bundle contains:
- **Personal Account statement** (PDF exported as PNGs) — reverse-chronological transaction list with a running (GBP) Balance column showing the personal account balance (excluding pots) after each transaction
- **Pot statements** (PDF exported as PNGs, one per pot) — each shows pot name, type, total balance at statement end, and a reverse-chronological list of deposits/withdrawals with running balance

The statement cover page shows three key totals: **Total balance** (personal + all pots), **Personal Account balance** (excluding pots), and **Balance in Pots**.

## Accounts in scope

| fynance account_id | Description |
|--------------------|-------------|
| `ope-monzo`        | Monzo personal account + all pots |

## Step 1 — Read all source images

Read all PNGs in parallel. The personal account statement pages run in reverse date order. Pot statement pages follow after. Identify:
- All pot names and types (e.g. "Dinero - EF Pot", "Sisters", "Next Day Withdraw. Don't Use!")
- Whether each pot is still open or was closed (closed pots show a "This Pot was closed on DD/MM/YYYY" notice)

## Step 2 — Derive end-of-month balances per pot

The balance column shows balance **after** each transaction. To find the end-of-month balance for a given pot:
- Find the last transaction on or before the last day of the month
- Its balance column value is the end-of-month balance for that pot

For the personal account, do the same from the personal statement pages.

End-of-month **total** = personal balance + sum of all pot balances at that date.

Verify against the cover page total (e.g. Apr 26 total £2,957.18 must match personal + pots).

## Step 3 — Post transactions

Write all transactions to a Python script file first (do not attempt inline curl heredocs — quoting breaks with merchant names containing apostrophes or special characters).

Use `POST /api/import`:

```json
{
  "account_id": "ope-monzo",
  "transactions": [
    {
      "date": "2026-01-26T01:46:25",
      "description": "JODA O",
      "amount": "4500.00",
      "currency": "GBP",
      "category_id": "<uuid>",
      "notes": "SALARY"
    }
  ]
}
```

Rules:
- `amount`: positive = money in, negative = money out
- `category_id`: must be a valid UUID from the categories table — resolve via `GET /api/categories` first
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
- `_CASH_DINERO` — Dinero EF pot (or whichever savings pot)
- `_CASH_SISTERS` — Sisters pot
- `_CASH_NEXT_DAY` — Next Day Withdraw pot (closed)
- Use `_CASH_<SHORTNAME>` for any new pots

```json
{
  "account_id": "ope-monzo",
  "holdings": [
    {
      "account_id": "ope-monzo",
      "symbol": "_CASH_MAIN",
      "name": "Current Account",
      "holding_type": "cash",
      "quantity": "1",
      "price_per_unit": null,
      "value": "6237.98",
      "currency": "GBP",
      "as_of": "2026-01-31T00:00:00",
      "is_closed": false
    }
  ]
}
```

Rules:
- Post one snapshot per pot per month-end (use `T00:00:00` for `as_of`)
- For a pot closed mid-month, post its final snapshot with `"value": "0.00"` and `"is_closed": true` using the actual close date as `as_of`
- Historical snapshots that predate a pot's closure should have `"is_closed": false` — only the final zero-balance snapshot is closed
- After importing, PATCH the closed pot: `PATCH /api/holdings/ope-monzo/_CASH_NEXT_DAY` with `{"is_closed": true, "as_of": "YYYY-MM-DD"}`
- This endpoint upserts — safe to re-run

## Step 5 — Verify

```bash
curl "http://127.0.0.1:7433/api/holdings?account_id=ope-monzo"
```

Expected: only open pots appear, each at their latest snapshot. Closed pots are excluded. If a pot appears that should be closed, check that its latest snapshot (by `as_of`) has `is_closed = true`.

## Known bugs fixed during this session

- `get_holdings_batch` originally used `MAX(as_of)` scoped to the whole account rather than per symbol — meant symbols with older snapshots than the newest symbol were dropped. Fixed by correlating the subquery on `symbol` and `sub_account`.
- `get_holdings_batch` originally filtered `AND h.is_closed = 0` before the `as_of` subquery — meant a closed pot's last snapshot was excluded and the previous open snapshot showed instead. Fixed by moving the `is_closed = 0` filter to apply after the latest-snapshot join, so closed pots are excluded entirely rather than showing stale open data.
- `delete_holding` matched `as_of` as an exact string — but handler passed `YYYY-MM-DD` while DB stores `YYYY-MM-DDTHH:MM:SS`. Fixed by using `DATE(as_of) = ?` in the SQL.
