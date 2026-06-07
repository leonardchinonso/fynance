# UK Capital Gains Tax Engine

Reference for the CGT calculation engine and its two read endpoints. The engine is HMRC-compliant for the three matching rules and produces a per-disposal breakdown suitable for an SA108 worksheet.

All TypeScript types referenced below are auto-generated from Rust via `ts-rs` and live in `frontend/src/bindings/`. Source of truth: [backend/src/server/routes/capital_gains.rs](../../backend/src/server/routes/capital_gains.rs).

---

## How It Works

```
   investment_events                  CGT engine (per request)               JSON response
 ┌──────────────────┐         ┌──────────────────────────────────┐         ┌─────────────────┐
 │ All buy/sell/    │  read   │ 1. Exclude ISA / Pension events  │         │ summary         │
 │ vest/withhold/   │ ──────> │ 2. Truncate at `as_at`           │ ──────> │ symbol_summaries│
 │ split/transfer   │         │ 3. Group by symbol               │         │ realized_events │
 │ rows in SQLite   │         │ 4. Same-day FIFO match           │         │ pools           │
 └──────────────────┘         │ 5. 30-day Bed & Breakfast match  │         └─────────────────┘
                              │ 6. S104 pool chronological replay│
                              │ 7. Emit per-match realized rows  │
                              │ 8. FX-convert totals to preferred│
                              └──────────────────────────────────┘
```

Every request replays the full event ledger. Nothing is cached on disk: there are no `s104_pools` or `cgt_disposals` tables. Editing or deleting an `investments` row automatically updates every downstream figure with no invalidation work.

---

## Inputs

The engine reads three tables on every request:

| Source | Used for |
|--------|----------|
| `investments` | The event ledger. One row per buy/sell/vest/withhold/split/transfer. |
| `accounts` | ISA and Pension accounts are identified by `account_type` and excluded from pool and CGT figures. |
| `currencies` | Loaded into `FxRateMap` to normalize per-disposal totals into the user's preferred currency for the top-level `summary`. |

Event types and their CGT semantics:

| Event type | CGT meaning |
|------------|-------------|
| `buy`, `vest` | Acquisition. Enters the S104 pool unless matched by the same-day or 30-day rules first. For RSU vests, the vest-date price is the acquisition cost. |
| `sell`, `withhold` | Disposal. Matched against acquisitions in rule order. `withhold` represents employer shares retained at vest to cover income tax (HMRC treats this as an immediate disposal at vest price). |
| `split` | Adjusts pool share count. `quantity` is interpreted as the split ratio (forward split: ratio > 1). Pool cost is unchanged; average cost per share is scaled accordingly. |
| `transfer` | No-op for CGT. Pool state is global per symbol so account-to-account moves do not change anything. |

---

## Matching Algorithm

For each symbol group, the engine applies HMRC's three rules in strict order. A disposal's `quantity` may be split across multiple rules. Any leftover after all three rules is emitted as `"Unmatched"` (short sale or data gap).

### 1. Same-day rule

For each calendar date where both an acquisition and a disposal occur, match them FIFO. The acquisition cost is the price recorded on the acquisition event.

### 2. 30-day rule (Bed & Breakfast)

For each remaining disposal `D`, match against acquisitions occurring in days `D+1` to `D+30` inclusive, FIFO. Acquisitions on `D` itself were already consumed by the same-day rule.

### 3. S104 pool

A chronological replay through the per-symbol event list maintains two running totals: `pool_shares` and `pool_cost` (in the trade's native currency). Acquisitions not consumed by the same-day or 30-day rules contribute `entering * price_per_share + proportional_fee` to `pool_cost` and `entering` to `pool_shares`. Disposals consume `matched * (pool_cost / pool_shares)` from the pool.

The pool is global per symbol across all non-sheltered accounts. Holding 50 AAPL in a Trading 212 GIA and 20 AAPL in a Freetrade GIA produces a single pool of 70 AAPL.

### 4. Unmatched remainder

If a disposal still has quantity left after all three rules (genuine short sale, or missing acquisition records), it is emitted with `cost_basis = 0`, `rule_applied = "Unmatched"`, and the full proceeds counted as a gain. This is a flag, not a recommendation.

---

## API

### `GET /api/investments/pools`

Returns the current S104 pool state per symbol. Useful for "what does my pool look like right now" or "what did it look like on date X".

**Query parameters:**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `as_at` | string (`YYYY-MM-DD`) | No | Replay only events up to and including this date. Omit for the current state. |

**Response:** `Array<S104PoolState>` — one entry per symbol that has ever appeared in the ledger (including pools whose `current_shares` is now zero).

```typescript
type S104PoolState = {
  symbol: string;
  current_shares: string;              // decimal as string
  total_allowable_expenditure: string; // decimal as string, native currency
  average_cost_per_share: string;      // decimal as string, native currency
};
```

ISA and Pension accounts are excluded. `total_allowable_expenditure` and `average_cost_per_share` are in the symbol's trading currency, not the user's preferred currency.

### `GET /api/investments/capital-gains`

Returns a full CGT report for a date range, including the disposal schedule and per-symbol breakdown.

**Query parameters:** All optional. If `tax_year` is provided, it takes precedence over `start_date`/`end_date`. Omit all three for a lifetime view.

| Field | Type | Description |
|-------|------|-------------|
| `tax_year` | string (`YYYY-YY` or `YYYY-YYYY`) | UK tax year, e.g. `2024-25` resolves to `6 Apr 2024` – `5 Apr 2025`. |
| `start_date` | string (`YYYY-MM-DD`) | Custom range start. |
| `end_date` | string (`YYYY-MM-DD`) | Custom range end. |
| `as_at` | string (`YYYY-MM-DD`) | Truncate the event replay at this date (point-in-time view). |
| `account_id` | string | Restrict the *fetched* events to one account. Pool math still treats the symbol globally across all accounts the events came from. |
| `symbol` | string | Restrict the *fetched* events to one symbol. |

**Response:**

```typescript
type CapitalGainsResponse = {
  summary: CgtSummary;
  symbol_summaries: SymbolSummary[];
  realized_events: CgtRealizedEvent[];
  pools: S104PoolState[];
};

type CgtSummary = {
  total_proceeds: string;        // decimal as string, preferred currency
  total_allowable_costs: string;
  total_gains: string;
  total_losses: string;          // positive number (absolute losses)
  net_gain_loss: string;         // total_gains - total_losses
  base_currency: string;         // user's preferred currency code
};

type SymbolSummary = {
  symbol: string;
  total_proceeds: string;        // decimal as string, preferred currency
  total_allowable_costs: string;
  total_gains: string;
  total_losses: string;
  net_gain_loss: string;
  original_currency: string;     // the symbol's trading currency
};

type CgtRealizedEvent = {
  symbol: string;
  disposal_id: string;
  disposal_date: string;         // ISO 8601 datetime
  quantity: string;
  disposal_price: string;        // per share, native currency
  proceeds: string;              // matched quantity * disposal price, net of proportional fee, native currency
  cost_basis: string;            // matched quantity * matched acquisition price, native currency
  gain_loss: string;             // proceeds - cost_basis, native currency
  rule_applied: "Same-Day" | "30-Day Rule" | "S104 Pool" | "Unmatched";
  original_currency: string;     // the symbol's trading currency
  matches: CgtMatchDetail[];
};

type CgtMatchDetail = {
  acquisition_id: string | null;     // null for pool / unmatched
  acquisition_date: string | null;   // ISO datetime, or "S104 Pool", or null for unmatched
  quantity: string;
  price: string;                     // per share, native currency
};
```

Per-event values stay in the trade's native currency. Only `summary` and `symbol_summaries` are converted into the preferred currency via `FxRateMap`. Each `CgtRealizedEvent` corresponds to one matched bucket, so a single disposal split across multiple rules produces multiple rows (one per rule, plus optionally an "Unmatched" row).

---

## Stored vs Computed

| Concern | Storage | Notes |
|---------|---------|-------|
| Event ledger | `investments` table | Persistent, append-able, editable. Source of truth. |
| Account sheltering | `accounts.account_type` | `investment_isa` and `pension` rows are excluded by the engine. |
| Preferred currency + FX rates | `currencies` table | One rate per currency, manually maintained. |
| S104 pools | None | Computed per request by chronological replay. |
| CGT disposals | None | Computed per request. |
| Historical FX rates | None | Not stored. Aggregates use the current rate from `currencies`. |

At personal-finance scale (hundreds to low thousands of events) the full replay runs in single-digit milliseconds. Cache tables would add invalidation complexity for no measurable benefit.

---

## Known Limitations

1. **FX uses today's rate for all historical disposals.** HMRC requires each leg of a foreign-currency trade to be converted at the rate on the leg's date (acquisition cost at acquisition-date rate, disposal proceeds at disposal-date rate). The engine currently applies one snapshot rate from `currencies` to every event. This is correct for a "rough position" view but not suitable for HMRC filing on non-preferred-currency trades. Resolving this is a V2 concern requiring a date-keyed historical rate store separate from `currencies`.
2. **Unmatched disposals report `cost_basis = 0`.** All proceeds are counted as gain. Useful as a data-quality flag; not a tax position.
3. **`split` only scales `pool_shares`, not `pool_cost`.** Correct for forward splits where total cost is unchanged and per-share cost falls. Reverse splits need the ratio expressed as a fraction in `quantity`; consolidations have not been tested end-to-end.
4. **No CGT annual exempt amount, no rate-band logic.** The engine produces raw gains/losses. Applying the annual exemption and computing tax due is left to the consumer (UI or accountant).
5. **No persistence of generated SA108 worksheets.** Document generation is a planned frontend concern; the engine only emits the underlying data.
