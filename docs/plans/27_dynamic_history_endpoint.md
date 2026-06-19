# 27 — Dynamic time-series ("history") endpoint

Status: **Proposed** (design only; not yet built)

## Context

We have accreted several single-purpose, bespoke-shape history endpoints, each
re-implementing the same period-bucketing + carry-forward + FX-conversion
machinery and returning a different JSON shape:

| Endpoint | Shape | Aggregates |
|---|---|---|
| `GET /api/holdings/history` (`get_monthly_net_worth`) | `HoldingsHistoryRow { month, available_wealth, unavailable_wealth, total_wealth }` | holdings value, split by account **availability** |
| `GET /api/holdings/account-history` (`get_account_holdings_history`, PR #77) | `AccountHoldingHistoryRow { period, total, values: [{symbol, value}] }` + `symbols[]` | one account's holdings value, split by **symbol** |
| `GET /api/holdings/cash-flow` (`get_cash_flow`) | `HoldingsCashFlowMonth { month, income, spending }` | transactions, split by **direction** |
| `GET /api/holdings/balances` (`get_balances_in_range` / `get_balance_summary`) | `AccountSnapshot[]` / `BalanceDelta[]` | per-account balances over time |

Every one of these is "a value over time, optionally broken down by some
dimension." Each new variation (the next was nearly "history but per asset
class", "history but per institution") tempts another bespoke endpoint and
another bespoke `ts-rs` type, frontend hook, and chart adapter.

The realisation that motivates this plan: a **generic response shape** (a list
of named series of `period → value`) removes the polymorphism objection that
normally argues *against* one flexible endpoint. The request's `measure` /
`group_by` / filters change only *which series* come back and *what the values
are* — never the response **shape**. So one endpoint can subsume all of the
above without the "one path, many contracts" problem.

This also centralises behaviour we've had to fix per-chart (respecting the
selected date range, and rendering gaps before a series' first value instead of
plotting zero — see PR #77 and the `portfolio_history.tsx` range/QNaN fixes):
the endpoint emits range-aligned, gap-aware series so every chart gets it for
free.

## Goals

- One endpoint that returns a **time series** with a uniform shape.
- Dynamic **granularity** (monthly / quarterly / yearly), already a solved
  primitive via `generate_period_end_dates`.
- Dynamic **measure** (what is being summed each period).
- Dynamic **group_by** (which dimension splits the data into series).
- **Filtering** down to accounts / profile / symbols / categories.
- Range-aligned, **gap-aware** output (null before a series begins) so the
  x-axis always respects the requested range and the frontend stops
  re-deriving this.
- Reuse the existing building blocks; no new aggregation engine.

## Non-goals

- Not a general query language / GraphQL. A fixed, validated set of measures
  and dimensions only.
- Point-in-time breakdowns (the donut/`by_type`/`by_institution`/`by_asset_class`
  on `GET /api/holdings/summary`) stay where they are — those are a snapshot at
  `as_of`, not a series. (They do share the same dimension taxonomy, so the
  grouping code can be shared.)
- Not removing the existing endpoints in the same change — migration is phased
  (see below).

## Proposed API

```
GET /api/history
```

### Request (query params)

| Param | Type | Required | Notes |
|---|---|---|---|
| `start` | date `YYYY-MM-DD` | yes | inclusive range start; the x-axis always begins here |
| `end` | date `YYYY-MM-DD` | yes | inclusive range end |
| `granularity` | `monthly\|quarterly\|yearly` | yes | reuse `parse_granularity` |
| `measure` | `value\|cash_flow` | no (default `value`) | what to sum per period |
| `group_by` | see matrix | no (default `total`) | dimension that splits series |
| `account_ids` | CSV | no | restrict to these accounts |
| `profile_id` | string | no | restrict to a profile |
| `symbols` | CSV | no | restrict to these holdings (value measure) |
| `exclude_category_ids` | CSV | no | exclude categories (cash_flow measure) |
| `include_closed` | bool | no (default false) | include closed holdings |
| `top_n` | int | no | keep top N series by latest value, collapse the rest into an `Other` series |

### `measure` × `group_by` matrix

| measure | valid `group_by` | source |
|---|---|---|
| `value` (carry-forward holdings value, FX-converted) | `total`, `availability`, `account`, `account_type`, `asset_class`, `institution`, `holding` (symbol) | `get_holdings_for_summary(period_end, …)` per period |
| `cash_flow` (transaction in/out per period) | `total`, `direction` (income vs spending), `category` | `get_cash_flow` / transaction aggregation |

Invalid combinations (e.g. `measure=cash_flow&group_by=holding`, or
`group_by=holding` without it being meaningful) return `400 invalid_grouping`.
`group_by=holding` is intended to be used with an `account_ids` filter (it
subsumes today's `account-history`).

### Response (uniform shape)

```jsonc
{
  "preferred_currency": "GBP",
  "granularity": "monthly",
  "measure": "value",
  "group_by": "availability",
  "periods": ["2024-01", "2024-02", "2024-03"],   // x-axis labels, range-aligned
  "series": [
    { "key": "total",       "label": "Total",       "values": ["31500.00", "32100.00", "32800.00"] },
    { "key": "available",   "label": "Available",   "values": [null, "14300.00", "15000.00"] },
    { "key": "unavailable", "label": "Unavailable", "values": ["17500.00", "17800.00", "17800.00"] }
  ]
}
```

- `values` is index-aligned to `periods`. Decimals are strings (codebase
  convention, `rust_decimal` as TEXT).
- `null` means "this series had no data in that period" (before it started, or
  after a holding was closed) → the chart renders a gap, never a line at 0.
  This bakes in the PR #77 / range-fix behaviour.
- A `total` series is always included unless `group_by=total` (in which case the
  single series *is* the total).
- Series order is stable and sorted by latest value (so `top_n` / legend order
  is deterministic).

This single shape replaces `HoldingsHistoryRow`, `AccountHoldingHistoryRow`,
`HoldingsCashFlowMonth`, and the per-account `symbols[]` metadata.

## Backend design

New module, e.g. `backend/src/storage/history.rs` (or a section of `db.rs`), and
a route `routes::history::get_history` registered at `/history`.

Reuse, don't reinvent:

- **Periods:** `generate_period_end_dates(from, to, granularity)` already yields
  `(label, period_end)` for all three granularities — this is the x-axis.
- **Value carry-forward:** `get_holdings_for_summary(period_end, profile_id)`
  already returns the carried-forward holdings as of a date with
  `account_type` + `institution`; apply `account_ids` / `symbols` /
  `include_closed` filters on top. (PR #77's `get_account_holdings_history`
  filters `is_closed = 0` for the gap semantics — fold that in.)
- **FX:** `FxRateMap` + `CurrencyAggregator` (per series).
- **Dimension keying:** a small `Grouper` (enum or trait) maps one holding row →
  `(series_key, label)`:
  - `availability` → `is_available_account(account_type)`
  - `account_type` → `account_type.as_str()`
  - `asset_class` → `account_type_to_asset_class(account_type)`
  - `institution` → `row.institution`
  - `account` → `account_id` (label = account name)
  - `holding` → `symbol` (label = `short_name ?? symbol`)
  - `total` → constant `"total"`
  This is the same taxonomy `get_holdings_summary` already uses for its
  point-in-time breakdowns, so the grouping fn can be shared between the two.
- **Cash flow:** `get_cash_flow(start, end, profile_id, granularity, exclude_category_ids, fx)`
  already produces income/spending per period; `group_by=category` extends it to
  per-category series (the `transactions_by_category` aggregation, bucketed by
  period).

Algorithm (value measure):

```
periods = generate_period_end_dates(from, to, granularity)
seriesAcc: Map<series_key, { label, Map<period_index, Decimal> , firstSeen }>
for (i, (label, period_end)) in periods:
    rows = carried_forward_holdings(period_end, filters)
    for row in rows:
        (key, slabel) = grouper(row)
        convert + add row.value into seriesAcc[key][i]   (CurrencyAggregator)
    // total series accumulates across all rows too
emit periods + series, where each series.values[i] is:
    null  if i < series.firstSeen   (gap before first data point)
    else  the accumulated value (0 allowed once started)
apply top_n: rank series by last non-null value; collapse the rest into "Other"
```

Gap rule = the one we just shipped: a series' value is `null` for periods before
its first non-zero entry; once it has started, subsequent zeros are real `0`s.

### Validation

- `validate_date_range`, `parse_granularity`, `parse_date`, `split_csv_param`
  (all exist).
- Reject invalid `measure`×`group_by` combos with `400 invalid_grouping`.
- Cap series count (e.g. `group_by=account` with hundreds of accounts) — default
  `top_n` (say 12) when `group_by` is high-cardinality, configurable.

### Performance note

`get_monthly_net_worth` already calls `get_holdings_for_summary` once per period
(N correlated-subquery reads). For long ranges × fine granularity this is N
round-trips. Acceptable to start (it's what ships today), but flag a future
optimisation: a single windowed query that returns the carried value per
(series_key, period) in one pass, instead of per-period re-querying.

## ts-rs bindings

- `HistoryResponse { preferred_currency, granularity, measure, group_by, periods: string[], series: HistorySeries[] }`
- `HistorySeries { key: string, label: string, values: (string | null)[] }`

(Enums `Measure`, `GroupBy` can be `ts-rs`-exported string unions for the
frontend to build type-safe requests.)

## Frontend impact

- One client method `api.getHistory(req)` + one `useHistory(req)` hook.
- One generic `<HistoryChart>` that takes `{ periods, series }` and renders lines
  (Total emphasised). The existing `StyledLineChart` already accepts
  `(string | number | null)[]` and renders gaps with `connectNulls={false}`
  (added in PR #77), so it's ready.
- Collapses three current consumers into configs of one component:
  - Portfolio **History** view → `measure=value, group_by=availability`.
  - Per-account **history** chart (drill-down) → `measure=value, group_by=holding, account_ids=[id]`.
  - Cash-flow chart → `measure=cash_flow, group_by=direction`.
- Deletes the bespoke `aggregateHistory` / `formatPeriodLabel` / firstIdx gap
  logic now duplicated in `portfolio_history.tsx` and `account_history_chart.tsx`
  — the backend hands back range-aligned, gap-aware, labelled series.

## Relationship to existing endpoints & migration

Phased, non-breaking:

1. **Phase 1 — ship `/api/history` for `measure=value`** (`total`, `availability`,
   `account`, `account_type`, `asset_class`, `institution`, `holding`). Existing
   endpoints untouched.
2. **Phase 2 — `measure=cash_flow`** (`direction`, `category`).
3. **Phase 3 — migrate the frontend** (History view, per-account chart,
   cash-flow chart) onto `/api/history`; delete the duplicated frontend
   aggregation/gap code.
4. **Phase 4 — deprecate** `/holdings/history`, `/holdings/account-history`,
   `/holdings/cash-flow` (keep as thin wrappers over the new query for one
   release with a `Deprecation` header, then remove). `/holdings/balances`
   (per-account snapshots/deltas) and `/holdings/summary` (point-in-time
   breakdowns) stay — different concerns.

Update `docs/api.html` (route-coverage check) as each phase lands.

## Open questions / decisions to confirm before building

1. **Path & name:** `GET /api/history` (top-level, since it spans holdings value
   *and* cash flow) vs `GET /api/holdings/series` (scoped). Top-level reads
   better once `cash_flow` is in.
2. **Response orientation:** column-oriented (`periods[]` + `series[].values[]`,
   proposed — compact, chart-friendly) vs row-oriented
   (`rows[].{period, values{}}`, matches today's endpoints). Column-oriented is
   leaner over the wire and trivial for Recharts.
3. **`measure` scope:** include `cash_flow` now (Phase 2) or keep v1 to
   holdings `value` only and leave cash flow on its endpoint indefinitely?
4. **Currency:** preferred-currency only (today's behaviour) vs an optional
   `currency` override param (defer; ties into multi-currency plan 22).
5. **`top_n` default** for high-cardinality `group_by` (12?), and whether
   "Other" is opt-in or automatic.
6. **Filters reach:** do we want `exclude_category_ids` to also affect the
   `value` measure (it currently only makes sense for cash flow)?

## Risks

- **Combinatorial validation surface:** the `measure`×`group_by`×filters matrix
  needs clear rejection of nonsensical combos; keep the valid set small and
  table-driven.
- **Performance** for long ranges × fine granularity × high-cardinality grouping
  (see perf note) — mitigate with `top_n` and, later, a single-pass query.
- **Migration discipline:** must actually delete the old endpoints/frontend code
  in Phase 3–4, or this *adds* surface instead of consolidating.

## Why this is worth doing

It turns "add another bespoke history endpoint every time we want a new
breakdown" into "add a `group_by` variant", centralises the range/gap behaviour
we've now fixed twice, and shrinks four endpoints + four `ts-rs` types + three
frontend chart adapters into one of each — without the polymorphic-response
downside, because the shape is uniform.
