# Frontend-to-Backend Handover: API & Model Contract

This document captures every model, API endpoint, and data contract the frontend requires from the backend. It is produced by auditing all mock data, TypeScript types, and page-level API usage against the existing backend design docs (`docs/design/03_data_model.md`, `docs/design/04_portfolio_overview.md`, `docs/plans/08_mvp_phases_v2.md`).

Where the frontend has diverged from the original plan, this document notes the divergence, evaluates which is better, and gives a recommendation.

---

## Table of Contents

1. [Model Differences](#1-model-differences)
2. [New Models Not in Backend Plan](#2-new-models-not-in-backend-plan)
3. [API Endpoints: Full Contract](#3-api-endpoints-full-contract)
4. [Frontend Logic to Delegate to Backend](#4-frontend-logic-to-delegate-to-backend)
5. [Open Questions for Backend](#5-open-questions-for-backend)
6. [Summary Checklist](#6-summary-checklist)

---

## 1. Model Differences

### 1.1 Account: `profile_ids` field (NEW)

| | Backend Plan | Frontend |
|---|---|---|
| Field | Not present | `profile_ids: string[]` |

**Context**: The backend `Account` struct has no profile/owner field at all. The frontend added `profile_ids: string[]` to support joint accounts (an account owned by multiple profiles). This was flagged as an open question in `fynance-project-note.md` ("Joint accounts: `Account.profile_id` becomes `Account.profile_ids: string[]`, or a separate `account_owners` join table").

**Recommendation**: The frontend's `profile_ids: string[]` is the right call. It's simpler than a join table for the expected scale (tens of accounts, not millions). The backend should add this field to the Account model and schema:
```sql
-- Option A (simple): JSON array in Account
profile_ids TEXT NOT NULL DEFAULT '[]'  -- JSON array of profile IDs

-- Option B (normalized): join table
CREATE TABLE account_owners (
    account_id TEXT NOT NULL REFERENCES accounts(id),
    profile_id TEXT NOT NULL,
    UNIQUE(account_id, profile_id)
);
```
**Recommendation**: Option A (JSON array) for MVP simplicity. The frontend already sends/receives `string[]`.

### 1.2 Holding: `short_name` field (NEW)

| | Backend Plan | Frontend |
|---|---|---|
| Field | Not present | `short_name: string` |

**Context**: The backend `Holding` struct has `name` (e.g., "Vanguard FTSE All-World ETF") but no `short_name`. The frontend added `short_name` (e.g., "All-World") for use in chart legends, pie chart labels, and compact portfolio views where the full name doesn't fit.

**Recommendation**: Add `short_name` to the backend Holding model. This is a display concern but it's better to have the backend provide it consistently than to have the frontend try to derive abbreviations. The backend can auto-generate a default `short_name` from the `name` (first word or ticker) and let users override it.

```sql
ALTER TABLE holdings ADD COLUMN short_name TEXT;
-- Default: derive from symbol or first word of name
```

### 1.3 Budget: standing vs per-month (OPEN QUESTION RESOLVED) [SKIP]

| | Backend Plan | Frontend |
|---|---|---|
| Schema | Per-month: `UNIQUE(month, category)` | Per-month: `{ month, category, amount }` |
| Behavior | Open question in docs | Generates standing targets replicated per-month |

**Context**: The backend plan flagged this as an open question. The frontend mock data takes standing budget targets and replicates them for every month. This is effectively "standing budgets implemented as per-month rows."

**Recommendation**: Keep per-month in the schema (allows seasonal variation like higher December food budget), but add a `POST /api/budget/standing` endpoint that sets a budget for all future months. The frontend's current mock behavior (same targets every month) works well with per-month storage. No model change needed.

### 1.4 Account types: property and mortgage [SKIP]

| | Backend Plan | Frontend |
|---|---|---|
| AccountType | `checking, savings, investment, credit, cash, pension` | Same set, but uses `savings` for property and `credit` for mortgage |

**Context**: The frontend mocks a home value as `type: "savings"` (id: "home-value") and a mortgage as `type: "credit"` (id: "mortgage-alex"). The project notes flag "property" and "mortgage"/"liability" as potential new account types.

**Recommendation**: For MVP, the frontend's approach works. The mortgage-as-credit is slightly awkward (credit cards and mortgages behave differently), but adding `property` and `mortgage` types is a post-MVP concern. The frontend already handles credit balances as liabilities via the `AVAILABLE_TYPES` classification. **No change needed for MVP.** If the backend later adds `property` and `mortgage` types, the frontend AccountType union type just needs two new members.

### 1.5 Transaction model: fully aligned [SKIP]

The frontend `Transaction` type matches the backend plan exactly. All fields present, same types, same semantics. No changes needed.

### 1.6 PortfolioSnapshot model: fully aligned [SKIP]

The frontend `PortfolioSnapshot` matches the backend schema. No changes needed.

### 1.7 HoldingType: `cash` variant removed [SKIP]

| | Backend Plan | Frontend |
|---|---|---|
| Variants | `stock, etf, fund, bond, crypto, cash` | `stock, etf, fund, bond, crypto` |

**Context**: The backend plan includes `HoldingType::Cash` for uninvested cash in brokerage accounts. The frontend dropped it. This was flagged as an open question in the project notes.

**Recommendation**: Keep `cash` in the backend enum for completeness. The frontend doesn't use it now but may need it if brokerage accounts show cash balances separately. No frontend change needed since unused variants are harmless.

---

## 2. New Models Not in Backend Plan

### 2.1 Profile

```typescript
interface Profile {
  id: string   // e.g., "alex", "sam"
  name: string // e.g., "Alex", "Sam"
}
```

**Context**: The backend has no `profiles` table. The project notes mention "multi-profile data model" as an open question. The frontend uses profiles to filter accounts, transactions, and portfolio data by person. This is essential for the multi-person household use case (Alex and Sam sharing one fynance instance).

**Recommendation**: The backend needs a `profiles` table:
```sql
CREATE TABLE IF NOT EXISTS profiles (
    id   TEXT PRIMARY KEY,
    name TEXT NOT NULL
);
```
And the `accounts` table needs `profile_ids` (see 1.1 above). Alternatively, profiles could be a simple config-level concept (stored in `config.yaml`), not a DB table, since profiles are rarely created/deleted.

**Backend endpoint needed**: `GET /api/profiles`

### 2.2 SpendingGridRow (computed response type)

```typescript
interface SpendingGridRow {
  category: string
  section: string       // "Income" | "Bills" | "Spending" | "Irregular" | "Transfers"
  months: Record<string, string | null>  // YYYY-MM -> Decimal string
  average: string | null
  budget: string | null
  total: string | null
}
```

**Context**: This is the core data type for the Budget spreadsheet view. It pivots transactions by category and month, classifies each category into a section, and joins against budget targets. The frontend currently computes this entirely client-side from raw transactions and budgets.

**Recommendation**: This MUST be a backend-computed endpoint (see Section 4.1 for details). The backend should return this shape directly from `GET /api/budget/spending-grid`.

### 2.3 BudgetRow (computed response type)

```typescript
interface BudgetRow {
  category: string
  budgeted: string    // Decimal string
  actual: string      // Decimal string (absolute spending)
  percent: number     // actual / budgeted * 100
}
```

**Context**: Used for the budget progress bars view. Currently computed client-side by joining budgets with transaction aggregates.

**Recommendation**: The backend's `GET /api/budget/:month` should return this shape. The plan already describes this endpoint returning "budget vs actual per category." The backend just needs to match this specific response type.

### 2.4 PortfolioResponse (computed response type)

```typescript
interface PortfolioResponse {
  net_worth: string
  currency: string
  as_of: string
  total_assets: string
  total_liabilities: string
  available_wealth: string      // checking + savings + investment
  unavailable_wealth: string    // pension + property
  accounts: Account[]
  by_type: PortfolioBreakdownItem[]
  by_institution: PortfolioBreakdownItem[]
  by_sector: PortfolioBreakdownItem[]
}
```

**Differences from backend plan's `GET /api/portfolio` response**:

| Field | Backend Plan | Frontend |
|---|---|---|
| `available_wealth` | Not present | NEW: sum of checking + savings + investment + cash |
| `unavailable_wealth` | Not present | NEW: sum of pension (+ property future) |
| `by_sector` | Not present | NEW: breakdown by sector (Stocks, Pension, Cash, Other) |
| `monthly_snapshots` | Present in plan | REMOVED: frontend uses separate `getPortfolioHistory()` |
| `by_type[].label` | Uses `type` key | Frontend uses `label` key |
| `by_institution[].label` | Uses `institution` key | Frontend uses `label` key |

**Recommendation**: The frontend's version is better. The available/unavailable split is a key UX concept (liquid vs locked wealth). The `by_sector` breakdown adds value. Moving `monthly_snapshots` to a separate endpoint is cleaner (avoids an unbounded array in the main portfolio response). The backend should adopt the frontend's response shape.

### 2.5 PortfolioHistoryRow (NEW endpoint response)

```typescript
interface PortfolioHistoryRow {
  month: string               // YYYY-MM
  available_wealth: string
  unavailable_wealth: string
  total_wealth: string
}
```

**Context**: The backend plan has `GET /api/portfolio/history` returning `monthly_snapshots` with just `{ month, net_worth }`. The frontend needs the available/unavailable split per month to render the stacked area chart (two colored areas).

**Recommendation**: The backend should compute and return the available/unavailable split per month. This is a `GROUP BY month, account_type_classification` query. Much better done server-side than client-side.

### 2.6 CashFlowMonth (NEW endpoint response)

```typescript
interface CashFlowMonth {
  month: string    // YYYY-MM
  income: string   // Decimal string
  spending: string // Decimal string
}
```

**Context**: Not in the backend plan at all. The frontend needs this for the cash flow bar chart on the Portfolio page. Currently computed client-side by iterating all transactions.

**Recommendation**: The backend needs a `GET /api/cash-flow` endpoint. This is a simple `GROUP BY month` with `SUM(CASE WHEN amount > 0 ...)` query. The backend plan's Phase 4 design doc shows this exact SQL query (in `04_portfolio_overview.md`), it just wasn't surfaced as a named endpoint.

### 2.7 Granularity (query parameter)

```typescript
type Granularity = "monthly" | "quarterly" | "yearly"
```

**Context**: The frontend supports viewing data at monthly, quarterly, or yearly granularity. This affects the spending grid, portfolio history, and chart aggregation. Currently the frontend does all the period grouping client-side.

**Recommendation**: The backend should accept `?granularity=monthly|quarterly|yearly` as a query parameter on relevant endpoints and handle the aggregation server-side.

---

## 3. API Endpoints: Full Contract

This is the complete list of endpoints the frontend needs, with request/response types. Endpoints marked **NEW** are not in the backend plan. Endpoints marked **MODIFIED** differ from the plan.

### 3.1 Profiles

#### `GET /api/profiles` (NEW)

Returns all profiles.

**Response**:
```json
[
  { "id": "alex", "name": "Alex" },
  { "id": "sam", "name": "Sam" }
]
```

### 3.2 Transactions

#### `GET /api/transactions` (MODIFIED)

**Backend plan**: `?month=&category=&account=&page=&limit=`
**Frontend needs**: `?start=&end=&accounts=&categories=&search=&page=&limit=&profile_id=`

Key differences:
- `month` replaced with `start` + `end` (date range, not single month)
- `account` replaced with `accounts` (comma-separated, multi-select)
- `category` replaced with `categories` (comma-separated, multi-select)
- `search` added (free-text search across merchant, category, account, notes)
- `profile_id` added (filter by profile ownership)

**Recommendation**: The frontend's filter set is strictly better. Date ranges are more flexible than single months. Multi-select for accounts/categories is essential for the UI's filter toggles. Search is a key usability feature.

**Response** (unchanged from plan, just wrapped in pagination):
```json
{
  "data": [Transaction, ...],
  "total": 142,
  "page": 1,
  "limit": 25
}
```

#### `GET /api/transactions/categories` (ALIGNED)

Returns `string[]` of distinct categories. No changes.

#### `GET /api/transactions/accounts` (REMOVED)

**Backend plan**: Lists accounts with transaction counts.
**Frontend**: Uses `GET /api/accounts` instead (the general accounts endpoint).

**Recommendation**: Drop this endpoint. The frontend only needs the account list for filter dropdowns, and `GET /api/accounts` provides that. If transaction counts per account are needed later, add them as an optional field.

### 3.3 Budget

#### `GET /api/budget/:month` (ALIGNED)

Returns `BudgetRow[]` (see type definition in Section 2.3). The backend computes actual spending from transactions and joins against budget targets.

**Response**:
```json
[
  { "category": "Food: Groceries", "budgeted": "300.00", "actual": "278.42", "percent": 93 }
]
```

#### `GET /api/budget/spending-grid` (NEW -- CRITICAL)

This is the most important new endpoint. The entire Budget spreadsheet view depends on it.

**Request**: `?start=YYYY-MM-DD&end=YYYY-MM-DD&granularity=monthly|quarterly|yearly&profile_id=`

**Response**: `SpendingGridRow[]`
```json
[
  {
    "category": "Food: Groceries",
    "section": "Spending",
    "months": { "2026-01": "-278.42", "2026-02": "-312.10", "2026-03": null },
    "average": "-295.26",
    "budget": "300.00",
    "total": "-590.52"
  }
]
```

**Section classification**: The mapping from category to section (Income, Bills, Spending, Irregular, Transfers) should be user-configurable and stored in the database (see Q2 in Section 5). The backend uses this mapping when building the spending grid response. Default seed values:
- `"Income"`: categories starting with `Income`
- `"Bills"`: `Housing`, `Finance: Insurance`, `Entertainment: Streaming`
- `"Transfers"`: `Finance: Savings`, `Finance: Investment`
- `"Irregular"`: `Travel`
- `"Spending"`: everything else

**Why this must be backend**: Currently the frontend loads ALL transactions for the date range (potentially thousands), then loops through them to build a pivot table by category and month, joins budget data, computes averages and totals. This is O(transactions * months) work that belongs in SQL.

#### `POST /api/budget` (ALIGNED)

**Request**:
```json
{ "month": "2026-03", "category": "Food: Groceries", "amount": "300.00" }
```

### 3.4 Portfolio

#### `GET /api/portfolio` (MODIFIED)

**Request**: `?profile_id=`

**Response**: `PortfolioResponse` (see Section 2.4 for full type). Key additions vs plan:
- `available_wealth` and `unavailable_wealth`
- `by_sector` breakdown
- Remove `monthly_snapshots` (use separate endpoint)

#### `GET /api/portfolio/history` (MODIFIED)

**Request**: `?start=YYYY-MM-DD&end=YYYY-MM-DD&granularity=monthly|quarterly|yearly`

**Response**: `PortfolioHistoryRow[]` with available/unavailable split (see Section 2.5).

The backend plan returns `{ month, net_worth }`. The frontend needs `{ month, available_wealth, unavailable_wealth, total_wealth }`.

#### `GET /api/portfolio/snapshots` (NEW)

Per-account monthly balance snapshots for delta calculations in the accounts grid.

**Request**: `?start=YYYY-MM-DD&end=YYYY-MM-DD`

**Response**: `PortfolioSnapshot[]`
```json
[
  { "snapshot_date": "2026-01-01", "account_id": "monzo-current", "balance": "2800.00", "currency": "GBP" }
]
```

**Why needed**: The accounts grid shows balance deltas (e.g., "+$320 this period"). It needs the earliest and latest snapshot per account within the date range to compute this. Currently the frontend loads ALL snapshots and filters client-side.

**Recommendation**: The backend should support a query like `?start=&end=&summary=true` that returns just the first and last snapshot per account in the range, rather than every monthly row.

#### `GET /api/cash-flow` (NEW)

**Request**: `?start=YYYY-MM-DD&end=YYYY-MM-DD&profile_id=`

**Response**: `CashFlowMonth[]`
```json
[
  { "month": "2026-01", "income": "4500.00", "spending": "3200.00" }
]
```

Currently the frontend computes this by iterating all transactions. This should be a `GROUP BY month` SQL query.

#### `GET /api/holdings` (MODIFIED)

**Backend plan**: `GET /api/holdings/:account_id`
**Frontend needs**: Same, but also needs a "get all holdings across all accounts" mode.

The Portfolio page calls `getHoldings(accountId)` in a loop for every investment/pension account to build the "By Stock" pie chart. This is an N+1 query problem.

**Recommendation**: Support `GET /api/holdings?account_ids=id1,id2,id3` (batch) or `GET /api/holdings?profile_id=alex` (all holdings for a profile). The backend can return holdings grouped by account in a single query.

#### `GET /api/accounts` (MODIFIED)

**Backend plan**: `GET /api/transactions/accounts` (confusing path)
**Frontend needs**: `GET /api/accounts?profile_id=`

**Response**: `Account[]` with `profile_ids` field (see Section 1.1).

### 3.5 Export

#### `GET /api/export` (ALIGNED)

**Request**: `?year=&format=csv|md`

The frontend has a stub `exportData(format)` method. No changes to the plan needed.

---

## 4. Frontend Logic to Delegate to Backend

These are places where the frontend currently does heavy computation that should be backend endpoints. Each item includes a link to the frontend view it powers, so you can see exactly what the endpoint needs to produce.

> **Base URL**: `http://localhost:5174` (Vite dev server)

### 4.1 CRITICAL: Spending Grid Computation

**See it live**: [Budget spreadsheet, 5 years, quarterly](http://localhost:5174/budget?view=spreadsheet&preset=5-years&start=2021-04-12&end=2026-04-12&granularity=quarterly)

This is the main budget spreadsheet view. It shows a pivot table of every spending category as rows, with time periods as columns, grouped into collapsible sections (Income, Bills, Spending, Irregular, Transfers). Each cell shows how much was spent in that category for that period. It also shows per-row averages, budget targets, and section totals.

**What the frontend currently does**: The mock service loads ALL transactions for the entire date range (could be thousands for a 5-year view), then:
1. Loops through every transaction, grouping by category and month
2. Classifies each category into a section (Income, Bills, etc.)
3. Joins against budget targets to get the "budget" column
4. Computes averages and totals per row
5. If granularity is quarterly/yearly, further aggregates months into periods

This is ~80 lines of JavaScript doing what is essentially a SQL `GROUP BY category, month` with a `LEFT JOIN budgets`.

**Why it matters**: A user with 3 years of data viewing a 5-year range could be downloading 5,000+ transactions to the browser to produce a table with maybe 40 rows. The database can do this in milliseconds.

**Fix**: `GET /api/budget/spending-grid?start=&end=&granularity=&profile_id=` returns `SpendingGridRow[]` directly. The backend does the pivot, the join, and the aggregation.

Also see: [Budget charts, same data](http://localhost:5174/budget?view=charts&preset=5-years&start=2021-04-12&end=2026-04-12&granularity=quarterly) -- the stacked bar, line, and pie charts all consume the same `SpendingGridRow[]` data, so this one endpoint powers both views.

---

### 4.2 CRITICAL: Portfolio Response Computation

**See it live**: [Portfolio overview](http://localhost:5174/portfolio?view=overview&preset=last-12-months)

This is the main portfolio dashboard. It shows the headline net worth figure, an available/unavailable wealth split, income vs spending averages, a "By Stock" pie chart of all holdings across all accounts, and investment performance metrics (market growth vs new cash invested).

**What the frontend currently does**: The mock service loads all accounts, then:
1. Iterates every account to sum assets vs liabilities
2. Classifies each account as available (checking, savings, investment, cash, credit) or unavailable (pension)
3. Groups accounts by type, institution, and sector to produce three separate breakdowns
4. Computes percentages for each breakdown

This is the kind of aggregation the database is built for: `GROUP BY type`, `GROUP BY institution`, with `SUM(balance)`.

**Why it matters**: Every time the user switches profiles or changes the date range, the frontend re-downloads all accounts and re-runs all the aggregation. The backend can cache these breakdowns and compute them in a single query.

**Fix**: `GET /api/portfolio?profile_id=` returns the full `PortfolioResponse` with net worth, available/unavailable split, and all three breakdowns pre-computed.

Also see: [Portfolio charts](http://localhost:5174/portfolio?view=charts&preset=last-12-months) -- the pie charts for "By Type", "By Institution", "By Sector", and "By Stock" all come from this same response.

---

### 4.3 HIGH: Portfolio History Aggregation

**See it live**: [Portfolio history, quarterly](http://localhost:5174/portfolio?view=history&preset=5-years&start=2021-04-12&end=2026-04-12&granularity=quarterly)

This view shows a stacked area chart of wealth over time, with two layers: available wealth (green) and unavailable wealth (blue). Below it is a table with rows per period showing the values and period-over-period deltas.

**What the frontend currently does**: The mock service loads ALL portfolio snapshots (one row per account per month, potentially hundreds of rows for a multi-year range), then:
1. Joins each snapshot with its account to classify it as available or unavailable
2. Groups by month and sums the two categories
3. Returns `{ month, available_wealth, unavailable_wealth, total_wealth }` per month

**Why it matters**: A user with 13 accounts over 3 years has 13 x 36 = 468 snapshot rows. The frontend downloads all of them just to produce ~36 data points for the chart. The backend can do this with a single `GROUP BY month` query joined on the accounts table.

**Fix**: `GET /api/portfolio/history?start=&end=&granularity=` returns pre-aggregated `PortfolioHistoryRow[]` with the available/unavailable split already computed.

---

### 4.4 HIGH: Cash Flow Computation

**See it live**: [Portfolio overview, scroll to "Income & Spending" section](http://localhost:5174/portfolio?view=overview&preset=last-12-months)

The portfolio overview shows average monthly income and average monthly spending figures, derived from transaction data. The cash flow data is also available for future chart views.

**What the frontend currently does**: The mock service iterates ALL transactions in the date range, splits them into positive (income) and negative (spending), and groups by month.

**Why it matters**: Same issue as the spending grid. Downloading thousands of transactions to compute monthly totals that are a simple SQL `GROUP BY substr(date, 1, 7)` with `SUM(CASE WHEN amount > 0 ...)`.

**Fix**: `GET /api/cash-flow?start=&end=&profile_id=` returns pre-aggregated `CashFlowMonth[]`. The SQL query is already documented in `docs/design/04_portfolio_overview.md`.

---

### 4.5 HIGH: Budget Row Computation

**See it live**: [Budget spreadsheet, single month view](http://localhost:5174/budget?view=spreadsheet&preset=this-month&granularity=monthly)

When viewing a single month, the budget page shows progress bars for each category: how much was budgeted vs how much was actually spent, with color coding (green/amber/red).

**What the frontend currently does**: The mock service filters the budgets table for the requested month, then iterates ALL transactions in that month to compute actual spending per category, then joins the two.

**Why it matters**: Even for a single month, the frontend downloads every transaction just to compute category totals. This is a textbook `GROUP BY category WHERE month = ?` query.

**Fix**: `GET /api/budget/:month` returns pre-joined `BudgetRow[]` where each row has `{ category, budgeted, actual, percent }`.

---

### 4.6 MEDIUM: Holdings N+1 Query

**See it live**: [Portfolio overview, "By Stock" pie chart](http://localhost:5174/portfolio?view=overview&preset=last-12-months) and [Portfolio charts, "By Stock" section](http://localhost:5174/portfolio?view=charts&preset=last-12-months)

The "By Stock" pie chart shows holdings aggregated across ALL investment and pension accounts. For example, if both Alex and Sam hold VWRL in different accounts, their values are summed into one "All-World" slice.

**What the frontend currently does**: The Portfolio page identifies all accounts with `type === "investment"` or `type === "pension"`, then calls `api.getHoldings(accountId)` in a loop for each one. If the user has 5 such accounts, that's 5 sequential API calls.

**Why it matters**: This is a classic N+1 query problem. Each API call has round-trip overhead. The backend can return all holdings for a profile in a single `SELECT ... WHERE account_id IN (...)` query.

**Fix**: Support `GET /api/holdings?profile_id=alex` or `GET /api/holdings?account_ids=a,b,c` to return all holdings in one call.

---

### 4.7 MEDIUM: Transaction Search

**See it live**: [Transactions with search](http://localhost:5174/transactions?view=table&preset=last-12-months&search=lidl&page=1)

The transactions table has a search box that filters across merchant name, description, category, account, and notes.

**What the frontend currently does**: The mock service loads all transactions matching the date/account/category filters, then does client-side `string.includes()` matching across 5 fields.

**Why it matters**: For a search like "lidl", the frontend downloads ALL transactions in the date range and then filters in JavaScript. The backend can do `WHERE normalized LIKE '%lidl%' OR description LIKE '%lidl%' OR ...` and return only matching rows. For even better performance, SQLite supports FTS5 full-text search.

**Fix**: `GET /api/transactions?search=lidl` does server-side search. The backend handles the LIKE/FTS query and returns only matching results, already paginated.

---

### 4.8 MEDIUM: Transaction Chart Aggregation (Dual-Fetch)

**See it live**: [Transaction bar chart](http://localhost:5174/transactions?view=bar&preset=last-12-months) and [Transaction pie chart](http://localhost:5174/transactions?view=pie&preset=last-12-months)

The bar and pie charts show spending broken down by category for the selected filters.

**What the frontend currently does**: On every filter change, the Transactions page makes TWO API calls:
1. `getTransactions({ page, limit: 25 })` for the paginated table
2. `getTransactions({ page: 1, limit: 10000 })` to download ALL matching transactions for chart aggregation

The second call downloads up to 10,000 transactions to the browser, then loops through them to compute `{ category: total }` for the charts.

**Why it matters**: The charts only need ~15-20 rows of aggregated data (`{ category, total }`), but the frontend downloads thousands of full transaction objects (each with 15+ fields) to compute those totals. This is the biggest bandwidth waste in the frontend.

**Fix**: Add a dedicated aggregation endpoint:
- `GET /api/transactions/by-category?start=&end=&accounts=&categories=&search=&profile_id=` returns `[{ category: "Food: Groceries", total: "1234.56" }]`
- The table continues to use the paginated `GET /api/transactions` endpoint
- The frontend never needs to download all transactions

---

### 4.9 LOW: Category List Derivation

**See it live**: [Transactions page category filter dropdown](http://localhost:5174/transactions?view=table&preset=last-12-months)

The transactions page has a category multi-select dropdown that lists all available categories.

**What the frontend currently does**: The mock service iterates all transactions and collects distinct `category` values into a `Set`.

**Why it matters**: Minor issue, but this means the dropdown only shows categories that have at least one transaction. If the user has never bought a flight, "Travel: Flights" won't appear. The backend has the canonical taxonomy in `config/categories.yaml` and should return the full list.

**Fix**: `GET /api/categories` returns the full taxonomy. The existing `GET /api/transactions/categories` endpoint could also work, but should return all categories from the taxonomy, not just those with data.

---

### 4.10 LOW: Account Snapshot Deltas

**See it live**: [Portfolio accounts grid](http://localhost:5174/portfolio?view=accounts&preset=last-12-months)

Each account card in the grid shows a balance delta: e.g., "+$320" or "-$150" compared to the start of the selected period. This tells the user at a glance which accounts grew and which shrank.

**What the frontend currently does**: The portfolio page calls `api.getAccountSnapshots(start, end)` which returns ALL monthly snapshots for all accounts in the range. Then the accounts grid component finds the earliest and latest snapshot per account and subtracts.

**Why it matters**: For 13 accounts over 12 months, that's 156 snapshot rows downloaded when only 2 per account (26 total) are needed. For a 5-year view, it's 780 rows.

**Fix**: `GET /api/portfolio/snapshots?start=&end=&summary=true` returns just `{ account_id, start_balance, end_balance, delta }` per account. The backend does:
```sql
-- First and last balance per account in range
SELECT account_id,
  FIRST_VALUE(balance) OVER (PARTITION BY account_id ORDER BY snapshot_date) as start_balance,
  LAST_VALUE(balance) OVER (PARTITION BY account_id ORDER BY snapshot_date
    RANGE BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING) as end_balance
FROM portfolio_snapshots
WHERE snapshot_date BETWEEN ? AND ?
```

---

### 4.11 LOW: Investment Metrics Computation

**See it live**: [Portfolio overview, "Investment Performance" card](http://localhost:5174/portfolio?view=overview&preset=last-12-months)

The overview shows investment performance metrics: total growth, new cash invested, and market growth (the difference). This tells the user how much of their portfolio growth came from market appreciation vs new deposits.

**What the frontend currently does**: The portfolio page fetches ALL transactions with `category: "Finance: Investment Transfer"` (up to `limit: 10000`), then sums them to get "new cash invested." It separately computes start/end investment account balances from snapshots. Market growth = balance change - new cash invested.

**Why it matters**: Downloads potentially thousands of transfer transactions to compute a single number. The backend can do `SUM(amount) WHERE category = 'Finance: Investment Transfer' AND date BETWEEN ? AND ?` in one query.

**Fix**: Include `investment_metrics` in the `GET /api/portfolio` response:
```json
{
  "investment_metrics": {
    "start_value": "30000.00",
    "end_value": "42850.00",
    "total_growth": "12850.00",
    "new_cash_invested": "8000.00",
    "market_growth": "4850.00"
  }
}
```
The backend computes all three values server-side from snapshots + transactions in a couple of queries.

---

## 5. Open Questions for Backend

These are decisions that need Nonso's input. Where the frontend team has a preference, it's noted, but the final call is Nonso's since these affect backend architecture.

### Q1: Profile storage -- DB table vs config file

**Decision needed by**: Nonso

The frontend needs a `Profile` model: `{ id: string, name: string }`. Profiles are created rarely (once per household member). Two options:

**Option A: DB table** (Ope's preference)
```sql
CREATE TABLE IF NOT EXISTS profiles (
    id   TEXT PRIMARY KEY,   -- e.g. 'alex'
    name TEXT NOT NULL       -- e.g. 'Alex'
);
```
Pros: Future-proof for profile-specific settings, avatars, preferences. Standard CRUD via API. Consistent with the rest of the data model.

**Option B: Config file**
```yaml
# config.yaml
profiles:
  - id: alex
    name: Alex
  - id: sam
    name: Sam
```
Pros: Simpler. No migration needed. Profiles change so rarely that a config file is sufficient. Avoids another table for what's essentially static data.

Either way, the frontend needs `GET /api/profiles` to return `[{ id, name }]`.

### Q2: Section classification for spending grid -- user-configurable mapping

**Decision needed by**: Nonso (implementation approach)

The budget spreadsheet groups categories into sections: Income, Bills, Spending, Irregular, Transfers. The frontend currently hardcodes this classification via prefix matching:
- `"Income"`: categories starting with `Income`
- `"Bills"`: `Housing`, `Finance: Insurance`, `Entertainment: Streaming`
- `"Transfers"`: `Finance: Savings`, `Finance: Investment`
- `"Irregular"`: `Travel`
- `"Spending"`: everything else

**Ope's decision**: This should be a user-configurable mapping stored in the database. Users should be able to move categories between sections (e.g., "I consider Gym a bill, not spending"). The mapping is: section name -> list of categories that belong to it.

**Suggested schema**:
```sql
CREATE TABLE IF NOT EXISTS section_mappings (
    section   TEXT NOT NULL,   -- 'Income', 'Bills', 'Spending', 'Irregular', 'Transfers'
    category  TEXT NOT NULL,   -- 'Housing: Rent / Mortgage', 'Food: Groceries', etc.
    UNIQUE(category)           -- each category belongs to exactly one section
);
```

The backend seeds this table with sensible defaults (the same rules the frontend uses today) and exposes endpoints to read/update the mapping. The spending grid endpoint uses this mapping when classifying rows.

### Q3: Budget model -- standing targets vs per-month rows

**Decision needed by**: Nonso

The frontend currently replicates the same budget targets for every month. The question is how the backend should model this. Three options:

**Option A: One row per month per category (current schema)**
```sql
-- budgets table: UNIQUE(month, category)
INSERT INTO budgets (month, category, amount) VALUES ('2026-01', 'Food: Groceries', '300.00');
INSERT INTO budgets (month, category, amount) VALUES ('2026-02', 'Food: Groceries', '300.00');
INSERT INTO budgets (month, category, amount) VALUES ('2026-03', 'Food: Groceries', '300.00');
-- ...one row for every month you want a budget for
```
Pros: Simple queries (`WHERE month = ?`). Allows different targets per month (e.g., higher December food budget). Each month is explicit.
Cons: Requires creating rows for every future month. If the user sets a budget in January and never touches it, do February-December rows exist? Who creates them?

**Example use case where this is better**: "I want to budget $500 for Travel in December but $0 every other month." Each month has its own row, so this is natural.

**Option B: Standing targets only (no month column)**
```sql
-- budgets table: UNIQUE(category)
INSERT INTO budgets (category, amount) VALUES ('Food: Groceries', '300.00');
```
Pros: One row per category, ever. Querying the budget for any month returns the same target. No need to pre-populate future months.
Cons: Cannot vary by month. "Higher food budget in December" requires a different mechanism.

**Example use case where this is better**: "I spend about $300/month on groceries. That's my target every month." One row, done.

**Option C: Standing targets with per-month overrides (Ope's preference)**
```sql
-- standing_budgets: the default for any month
INSERT INTO standing_budgets (category, amount) VALUES ('Food: Groceries', '300.00');

-- budget_overrides: per-month exceptions
INSERT INTO budget_overrides (month, category, amount) VALUES ('2026-12', 'Food: Groceries', '500.00');
```
The backend resolves: "For December 2026 Food: Groceries, return 500 (override). For January 2026, return 300 (standing)."
Pros: Best of both worlds. Most categories use standing targets, seasonal exceptions are explicit.
Cons: Two tables, slightly more complex query logic.

**Example**: User sets standing budget of $300/month for groceries, then overrides December to $500 because of holiday cooking. Every other month returns $300 automatically.

Ope leans toward Option C but this is ultimately a backend architecture decision. The frontend doesn't care which option is used, as long as `GET /api/budget/:month` returns the effective budget rows for that month.

### Q4: Granularity aggregation strategy

**Decision needed by**: Nonso (confirmation)

When the frontend requests data with `?granularity=quarterly` or `?granularity=yearly`, the backend needs to aggregate monthly data into larger periods. The correct aggregation strategy differs by endpoint:

**Spending/cash flow endpoints** (spending grid, cash flow, budget): **SUM** across months in the period.
- Example: Q1 2026 spending on groceries = Jan ($278) + Feb ($312) + Mar ($295) = **$885**
- Why: Spending accumulates. You want to know "how much did I spend this quarter total."

**Portfolio/balance endpoints** (portfolio history, snapshots): **LAST VALUE** in the period.
- Example: Q1 2026 net worth = March 2026 value = **$131,400** (not Jan + Feb + Mar summed)
- Why: Balances are point-in-time. You want to know "what was my net worth at the end of this quarter," not the sum of three months of net worth.

**Endpoints affected**:
- `GET /api/budget/spending-grid?granularity=quarterly` -- SUM strategy
- `GET /api/cash-flow?granularity=quarterly` -- SUM strategy
- `GET /api/portfolio/history?granularity=quarterly` -- LAST VALUE strategy

Ope and I believe this is correct. Nonso, please confirm or adjust if the backend has a different view.

### Q5: Historical portfolio queries via `as_of`

**Decided**: Yes, support this.

The backend should support `GET /api/portfolio?as_of=2026-01-31` to return the portfolio state as of any past date. This uses the carry-forward semantics already described in `design/03_data_model.md` (Key Decision #5): for each account, return the most recent balance on or before the `as_of` date.

When `as_of` is omitted, default to today (or the most recent balance update date). The `as_of` field in the response should reflect the actual date used.

---

## 6. Summary Checklist

### New DB tables needed
- [ ] `profiles` (id, name)
- [ ] `account_owners` or add `profile_ids` JSON column to `accounts`

### Model changes
- [ ] `Account`: add `profile_ids` field
- [ ] `Holding`: add `short_name` field

### New endpoints (not in backend plan)
- [ ] `GET /api/profiles`
- [ ] `GET /api/budget/spending-grid?start=&end=&granularity=&profile_id=`
- [ ] `GET /api/cash-flow?start=&end=&profile_id=`
- [ ] `GET /api/portfolio/snapshots?start=&end=`
- [ ] `GET /api/transactions/by-category?start=&end=&accounts=&profile_id=` (chart aggregation)

### Modified endpoints (differ from plan)
- [ ] `GET /api/transactions`: date range instead of single month, multi-select filters, search, profile_id
- [ ] `GET /api/portfolio`: add available/unavailable wealth, by_sector, remove monthly_snapshots
- [ ] `GET /api/portfolio/history`: return available/unavailable split, accept granularity
- [ ] `GET /api/holdings`: support batch/profile query (avoid N+1)
- [ ] `GET /api/accounts`: accept `?profile_id=` filter
- [ ] `GET /api/budget/:month`: return `BudgetRow[]` with pre-computed actual spending

### Endpoints aligned with plan (no changes needed)
- [ ] `POST /api/budget` (set budget amount)
- [ ] `POST /api/accounts` (register account)
- [ ] `PATCH /api/accounts/:id/balance` (update balance)
- [ ] `PATCH /api/transactions/:id` (edit category, notes)
- [ ] `GET /api/transactions/categories`
- [ ] `GET /api/export`

### Endpoints in plan but not yet used by frontend
- [ ] `POST /api/import` (typed JSON import)
- [ ] `POST /api/import/csv` (CSV upload)
- [ ] `POST /api/import/bulk` (multiple CSVs)
- [ ] `GET /api/income/:month` (not used; frontend derives from transactions)
- [ ] `GET /api/reports/:month` (Reports page is stub)
- [ ] `GET /api/docs` (OpenAPI spec)
- [ ] `GET /api/ingestion/checklist/:month`
- [ ] `POST /api/ingestion/checklist/:month/:account_id`
- [ ] Token management (CLI only)

### Frontend logic to move to backend (priority order)
1. [ ] **CRITICAL**: Spending grid computation (pivot by category x month)
2. [ ] **CRITICAL**: Portfolio response aggregation (net worth, breakdowns)
3. [ ] **HIGH**: Portfolio history aggregation (available/unavailable per month)
4. [ ] **HIGH**: Cash flow computation (income vs spending per month)
5. [ ] **HIGH**: Budget row computation (budget target + actual spending join)
6. [ ] **MEDIUM**: Holdings batch query (avoid N+1)
7. [ ] **MEDIUM**: Transaction search (server-side LIKE or FTS)
8. [ ] **MEDIUM**: Chart data aggregation (by-category totals, not raw transactions)
9. [ ] **LOW**: Category list from taxonomy, not transaction scan
10. [ ] **LOW**: Account snapshot deltas (first/last per account)
11. [ ] **LOW**: Investment metrics (new cash vs market growth)

---

## 7. Accounts vs Portfolio Snapshots vs Holdings: Analysis

This section captures findings from a frontend review of how the three portfolio-related concepts relate and where the current naming/design could be improved.

### The three concepts

| Concept | What it stores | Granularity | Applies to |
|---|---|---|---|
| **Account** | A container with a current balance, institution, type | One row per account | All account types |
| **Portfolio Snapshot** | Historical balance for one account at one date | One row per account per date | All account types |
| **Holding** | A named sub-balance within an account | Multiple rows per account per date | Currently: investment + pension only |

### How they link

All three are connected through `account_id`:

```
Account (parent)
  |-- PortfolioSnapshot[] (balance over time for this account)
  |-- Holding[]           (composition within this account)
```

Holdings and snapshots do not reference each other. A "portfolio view" is derived by aggregating snapshots across accounts. Holdings drill down into what an account is made of.

### Recommendation: rename `portfolio_snapshots` to `account_snapshots`

The current name `portfolio_snapshots` is misleading. Each row is a balance for **one account** at one date, not a portfolio-level aggregate. The frontend already treats it this way: the mock service method is `getAccountSnapshots()`, and portfolio-level numbers are derived by summing across accounts.

Renaming the table (and the Rust struct) to `account_snapshots` / `AccountSnapshot` would make the data model self-documenting:

- `accounts` = current state of each account
- `account_snapshots` = historical state of each account
- `holdings` = composition within an account

This is a low-risk rename since the table is only referenced in the storage layer. The API endpoint can remain `/api/portfolio/snapshots` if preferred (the URL describes the feature area, not the table).

### Recommendation: add `"cash"` to `HoldingType`

The current `HoldingType` enum is `stock | etf | fund | bond | crypto`. This limits holdings to securities, but there are real use cases for **cash holdings** within accounts:

1. **Uninvested cash in investment accounts**: A Trading 212 ISA might hold £500 in uninvested cash alongside stock positions. Without a cash holding, the account balance (£39,000) would not match the sum of holdings (£38,500), with no way to represent the gap.

2. **Pension cash allocations**: Some pension providers split funds between investment funds and a cash reserve. Both are part of the same pension account.

3. **Bank pots/goals**: Monzo pots, Revolut vaults, and Chase roundup accounts are sub-balances within a single bank account. Rather than modelling each pot as a separate account (which inflates the account count and loses the grouping), they can be represented as cash holdings within the parent account:
   ```
   Account: "Monzo Current" (balance: £2,500)
     |-- Holding: "Main balance"   (cash, £590)
     |-- Holding: "Bills pot"      (cash, £800)
     |-- Holding: "Holiday pot"    (cash, £600)
     |-- Holding: "Emergency pot"  (cash, £510)
   ```

4. **Multi-currency balances**: A Revolut account with GBP, EUR, and USD balances could represent each currency as a cash holding, preserving the per-currency detail within a single account.

Adding `"cash"` to `HoldingType` generalizes holdings from "securities in investment accounts" to "any named sub-balance within any account". This does not change the API design: accounts without sub-divisions simply have no holdings, and the account balance remains the source of truth for total value.

**Note on user choice**: Whether to model something as separate accounts or as holdings within one account (e.g., Monzo pots as accounts vs. cash holdings) is a user configuration decision at ingestion time. The API supports both patterns without changes.

### Proposal: consolidate account snapshots into holding-level history (eliminate separate snapshot table)

**Status**: Under consideration. Ope's strong preference. Needs Nonso's input on backend complexity.

Rather than maintaining two separate time-series tables (`account_snapshots` for balance history and `holdings` for point-in-time composition), consolidate into a single model where **holdings are the atomic unit of history** and account-level snapshots are derived by summing holdings.

**Current design (two tables)**:
```
account_snapshots: { account_id, snapshot_date, balance }     -- account-level time series
holdings:          { account_id, symbol, as_of, value, ... }  -- point-in-time composition
```

**Proposed design (one table)**:
```
holdings:          { account_id, symbol, as_of, value, ... }  -- holding-level time series
-- account balance at any date = SUM(holdings.value) WHERE account_id = X AND carry-forward to date
```

**Why consolidate**:

1. **Single source of truth**: Two tables that both describe "what an account is worth over time" will inevitably drift. If the account snapshot says £39,000 on March 1 but holdings sum to £38,500, which is right? With one table, there's no ambiguity.

2. **Build the right thing from the start**: We will almost certainly want holding history later (composition drift, performance attribution, "when did I buy NVDA"). Adding it retroactively means backfilling data or losing early history. Storing it from day one costs almost nothing.

3. **Simpler ingestion**: One code path writes holdings. Account balances are always derived. No need to keep two tables in sync during import.

4. **The carry-forward query is not hard**: To get an account balance as of a target date, carry forward each holding to that date and sum:
   ```sql
   SELECT SUM(h.value) as account_balance
   FROM holdings h
   WHERE h.account_id = ?1
     AND h.as_of = (
       SELECT MAX(h2.as_of) FROM holdings h2
       WHERE h2.account_id = h.account_id
         AND h2.symbol = h.symbol
         AND h2.as_of <= ?2  -- target date
     )
   ```
   At the scale of this app (tens of accounts, tens of holdings each), this is negligible.

**What this requires**:

1. **`"cash"` HoldingType becomes mandatory, not optional**. Every account needs at least one holding to have history. A simple checking account like Monzo with no subdivisions would have a single cash holding representing the whole balance:
   ```
   Account: "Monzo Current"
     └── Holding: "Monzo Current" (cash, £590, as_of: 2026-03-15)
   ```
   This is a bit redundant for simple accounts, but it's the cost of a unified model. The alternative (special-casing accounts with no holdings) reintroduces the two-source problem.

2. **The `account_snapshots` / `portfolio_snapshots` table can be dropped entirely**. The `accounts.balance` and `accounts.balance_date` fields can remain as a denormalized cache of the latest total, but the time-series history lives in holdings only.

3. **Ingestion always writes holdings, never snapshots**. When importing a Monzo CSV with a closing balance of £590, the importer creates a single cash holding: `{ account_id: "monzo-current", symbol: "GBP", holding_type: "cash", value: "590.00", as_of: "2026-03-15" }`. When importing a Trading 212 export, it creates one holding per position plus a cash holding for uninvested balance.

**Trade-offs**:

| | Current (two tables) | Proposed (holdings only) |
|---|---|---|
| Query for account balance at date | Single row lookup | Carry-forward + SUM across holdings |
| Query for net worth at date | SUM across account snapshots | Carry-forward + SUM across all holdings |
| Ingestion complexity | Write to two tables, keep in sync | Write to one table |
| Simple accounts (checking, savings) | One snapshot row per date | One cash holding row per date (slightly redundant) |
| Investment accounts | Snapshot + holdings (can drift) | Holdings only (single source of truth) |
| Future holding history | Needs backfill or new table | Already there |

**Ope's view**: The redundancy for simple accounts is a small price for a cleaner, single-source model. Building two parallel time-series systems and then likely wanting holding history anyway feels like unnecessary complexity. Better to get this right from the start.

**Decision needed from Nonso**: Is the carry-forward SUM query acceptable for the backend, or does the single-row lookup for account balances matter for performance? At this app's scale it shouldn't, but this is a backend architecture call.

### CSV import should create holdings (not just transactions)

Currently CSV import only creates transactions. But many bank CSV exports include a running or closing balance, and investment account exports may include positions. The import should extract all available data in one pass.

**If the consolidation proposal above is adopted** (holdings as the single source of balance history), the import flow becomes:

```
CSV Import
  |-- Always: create transactions (deduplicated by fingerprint)
  |-- If closing balance available: upsert a cash holding for that account
  |     e.g., { account_id: "monzo-current", symbol: "GBP", holding_type: "cash",
  |             value: "590.00", as_of: "2026-03-15" }
  |-- If holdings data available (investment/trading CSVs): upsert per-symbol holdings
  |     plus a cash holding for any uninvested balance
```

**If the current two-table design is kept**, the import flow is:

```
CSV Import
  |-- Always: create transactions (deduplicated by fingerprint)
  |-- If closing balance available: create an account snapshot row
  |-- If holdings data available: upsert holdings for the account
```

Either way, the key point is that a single CSV import should update both the transaction history and the balance/holdings history in one step, rather than requiring the user to manually set the balance after importing.

This is importer-specific logic. The LLM-based CSV parser already exists at `backend/src/importers/csv_importer.rs`. It would need to be taught to recognize balance columns and holdings data in addition to transactions. This is prompt engineering work on top of the existing importer (adjusting what the LLM extracts from the CSV), not a new importer. The API contract doesn't change, just the scope of what a single import operation produces.

### Currency: store in source currency, convert on display

All monetary values (transactions, account balances, snapshots, holdings) should be stored in their **source currency**, the currency the account is actually denominated in. Never convert at ingestion time, because exchange rates change and you would lose the original value.

This matters for several real scenarios:
- A Revolut USD account with dollar-denominated transactions
- A Nigerian bank account in Naira (NGN)
- Investment accounts holding US-listed stocks priced in USD but within a GBP-denominated ISA wrapper

The `currency` field already exists on transactions, accounts, snapshots, and holdings. The key requirement is that these are always populated with the actual source currency, not defaulted to GBP.

**Display-time conversion**: The frontend should support toggling between:
- **Source currency**: show the raw value as stored (e.g., "$1,200.00 USD")
- **Preferred currency**: convert to the user's preferred currency (e.g., "~£948.00 GBP") using a rate

For net worth aggregation across accounts in different currencies, the backend needs to convert to a common currency (the user's preferred currency, typically GBP).

**Approach: store source currency + ingestion-time exchange rate**

The principle is: store the value in its actual source currency (never convert the stored value), but also capture the exchange rate at ingestion time so historical views use historically accurate rates. This means each holding row stores both what it's worth in its native currency and how to convert it.

For MVP, the exchange rate source can be simple: a free API like exchangerate.host called at ingestion time, or a manually provided rate. The backend caches rates by `(currency, date)` so repeated ingestions on the same day don't make redundant API calls.

**Schema approach**: A lightweight `exchange_rates` reference table is cleaner than adding a rate column to every holdings row, since the same rate applies to all holdings in the same currency on the same date:

```sql
CREATE TABLE IF NOT EXISTS exchange_rates (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    from_currency   TEXT NOT NULL,        -- e.g., 'USD'
    to_currency     TEXT NOT NULL,        -- e.g., 'GBP' (user's preferred)
    rate            TEXT NOT NULL,        -- Decimal string
    rate_date       TEXT NOT NULL,        -- YYYY-MM-DD
    source          TEXT NOT NULL DEFAULT 'manual',  -- 'manual' | 'api'
    captured_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    UNIQUE(from_currency, to_currency, rate_date)
);
```

The frontend can then:
- Show source currency by default ("$1,200.00 USD")
- Toggle to preferred currency using the rate for that date ("~£948.00 GBP at rate captured on 2026-03-15")
- Aggregate net worth across currencies by joining holdings with `exchange_rates` on `(currency, as_of)`

**What this means for the schema**:
- `transactions.currency`: already exists, ensure it's always the source currency
- `accounts.currency`: already exists, same
- `holdings.currency`: already exists, same
- New: `exchange_rates` table (schema above)
- GBP-denominated accounts don't need rate rows (rate is 1.0 by definition, skip the lookup)

### Summary of recommended changes

| Change | Impact | Priority |
|---|---|---|
| **Consolidate snapshots into holdings** (Ope's preference) | Drop `portfolio_snapshots` table, use holdings as single time-series source. Account balance = SUM(holdings). Needs Nonso's sign-off. | **High (architectural, decide before building)** |
| Add `"cash"` to `HoldingType` enum | Required if consolidation is adopted (every account needs at least one holding). One-line Rust enum + schema change. | **High (required for consolidation)** |
| Rename `portfolio_snapshots` to `account_snapshots` | Only if consolidation is NOT adopted. Moot if table is dropped. | Low |
| CSV import: extract balance/holdings, not just transactions | Importer writes holdings (or snapshots) from closing balance and position data in CSVs | Medium |
| Store all values in source currency, never convert at ingestion | Convention/validation, no schema change needed | High (enforce from the start) |
| Add exchange rate capture at ingestion time | New column on holdings or separate `exchange_rates` table | Medium (needed for multi-currency net worth) |
| Frontend: toggle between source and preferred currency | UI feature | Low (post-MVP, after backend currency support) |
| Decide on balance/holdings mismatch handling (see Appendix A) | Backend validation logic. Less relevant if consolidation is adopted (holdings ARE the balance). | Medium |

---

## Appendix A: Account Balance vs Holdings Sum Mismatch

For accounts that have both a balance (from `accounts` or `account_snapshots`) and holdings, there is no guarantee that `SUM(holdings.value)` equals the account balance. This can happen legitimately:

- **Uninvested cash**: A Trading 212 ISA has £39,000 total but only £38,500 in stocks. The remaining £500 is uninvested cash sitting in the account. Without a cash holding (see the `"cash"` HoldingType recommendation above), the sum of holdings will always be less than the account balance.
- **Timing mismatch**: The account balance was updated on March 20 but holdings were last updated on March 15. Stock prices moved in between.
- **Fees/pending settlements**: The account shows a balance net of pending fees or unsettled trades that aren't reflected in the holdings snapshot.
- **Rounding**: Multiple holdings each rounded to 2 decimal places may not sum exactly to the rounded account balance.

### Options for handling this

**Option A: Soft warning (recommended for MVP)**

Accept the mismatch and surface it in the API response. When `GET /api/holdings?account_id=X` returns holdings, include a summary field:

```json
{
  "account_id": "t212-isa-alex",
  "account_balance": "39000.00",
  "holdings_total": "38500.00",
  "unaccounted": "500.00",
  "holdings": [...]
}
```

The frontend can display this as "£500 unaccounted (uninvested cash, timing differences, or pending updates)". This is informational, not blocking.

**Option B: Auto-create a cash holding for the gap**

When the mismatch is positive (account balance > holdings sum), automatically create a synthetic `"cash"` holding for the difference. This keeps `SUM(holdings.value) == account_balance` as an invariant, but introduces a "fake" holding that the user didn't explicitly create.

**Option C: Strict enforcement (reject mismatched updates)**

Reject holdings updates where the sum doesn't match the account balance. This is too rigid: it would block legitimate cases (timing mismatches, uninvested cash) and create friction during ingestion.

**Recommendation**: Option A for MVP. The mismatch is real information, not a bug. Surfacing it lets the user decide whether to add a cash holding to close the gap or leave it as-is. Option B could be offered as a user-facing toggle later ("auto-create cash holding for uninvested balance").

---

## Appendix B: Additional Design Concerns Raised During Review (2026-04-13)

These are items surfaced while auditing `origin/master` against this handover doc. They are not blockers for the current MVP, but most need a decision from Nonso before the relevant code path matures. Ope's notes inline where opinions already exist.

### B.1 Holdings carry-forward: ghost positions after a sale

**Concern:** Account balance at date `T` is computed by taking the latest holding row per symbol where `as_of <= T` and summing values. If the user sells a holding and simply stops reporting it, the old row carries forward forever, silently over-stating account value.

**Ope's view:** Expectation is the user will set a zero snapshot for the sold symbol, but that's easy to forget and hard to audit. It keeps reporting zero after that, which is also not useful and needs cleanup.

**Needs:** Some explicit mechanism — one of:
- A `closed_at` (or `as_of_end`) column on holdings so rows stop contributing after that date
- A convention that a value of `0` means "closed," plus a cleanup job
- A soft UI warning when a symbol hasn't been refreshed in N days

**Priority:** Low (correctness, not blocker). Mostly a "what does Nonso think?" item.

### B.2 Timestamps are naive (no timezone) through the whole stack

**Concern:** `Transaction.date` is `NaiveDateTime` with no timezone info. `util::fingerprint` hashes the date string directly, and `util::parse_naive_datetime` zeros date-only inputs to `T00:00:00`. The backend has no idea what timezone any of this is in, so semantics of "midnight" depend on the machine that did the ingest.

**Ope's view:** For self-hosted, the user will almost always ingest from home, so this is fine in practice. What matters is that once ingested, the stored value must not shift if the UI is loaded from a different timezone. The proposal:

1. Assume the user's local timezone at ingestion time (or let them provide one in config)
2. Stamp the timezone on the value as it's written to the DB
3. Downstream reads are TZ-aware and stable regardless of where the UI is loaded

If the current stored format already behaves this way (i.e. the string is interpreted consistently regardless of browser TZ), this is already fine and just needs a note in the docs. If not, it needs the ingest-time stamping above.

**Priority:** Low (self-hosted, usually same TZ) but worth confirming behavior before leaving it as-is.

### B.3 `profile_ids` stored as JSON-in-TEXT

**Concern:** `accounts.profile_ids` is a JSON array packed into a TEXT column. Can't index, can't enforce referential integrity against the `profiles` table, can't ask "which accounts does Alex own" without a `LIKE` scan.

**Ope's view:** Not a concern at this scale. Max ~20 accounts, self-hosted, one machine. Full table scans over 20 rows are free. Leaving as-is.

**Priority:** None (explicitly accepted).

### B.4 Transaction edits have no audit trail

**Concern:** `PATCH /api/transactions/:id` silently overwrites category and notes. `category_source` captures only the *current* source (`rule`/`agent`/`manual`), not history. If an agent mass-categorizes and a user then corrects some rows, there's no way to reconstruct "what did the agent originally say" or to revert a bad edit.

**Ope's view:** This feels important. At minimum there should be a log of old→new for category and notes edits so bad changes can be reverted. A full audit trail (with timestamps and who made the change) is nicer but the log is the floor.

**Suggested minimum:**
```sql
CREATE TABLE IF NOT EXISTS transaction_edits (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    transaction_id  TEXT NOT NULL,
    field           TEXT NOT NULL,     -- 'category' | 'notes'
    old_value       TEXT,
    new_value       TEXT,
    changed_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    changed_by      TEXT                -- 'user' | 'agent' | future: user id
);
```

The `PATCH` handler writes one row per changed field. A simple `GET /api/transactions/:id/history` exposes it for revertability.

**Priority:** Medium (correctness / trust).

### B.5 LLM parser calls Claude with raw statement content

**Concern:** The LLM-backed CSV importer sends raw bank statement content to the Anthropic API (via `FYNANCE_ANTHROPIC_API_KEY`). This is the one outbound call from an otherwise telemetry-free binary. Non-deterministic: the same CSV re-parsed can produce slightly different normalized fields (fingerprint-based dedup catches full duplicates, but not descriptions drifting).

**Ope's view:** Accepting this — there isn't a realistic alternative for free-form CSV parsing at MVP quality, and the user opts in by setting the API key. Just needs to be called out clearly in docs as the one exception to the "no outbound calls" line in CLAUDE.md.

**Action:** Documentation only. Update CLAUDE.md security section and the README to explicitly note that CSV imports leave the machine when the Anthropic key is configured.

**Priority:** Low (documentation).

### B.6 Fingerprint collapses same-day same-amount transactions

**Concern:** `fingerprint = sha256(datetime | amount | account_id)`. UK bank CSVs (Monzo, Revolut, Lloyds) generally don't include time-of-day — only the date — so `util::parse_naive_datetime` zeros date-only inputs to `T00:00:00`. Two £5 Pret coffees bought on the same day from the same account produce identical fingerprints and collapse into one on dedup. Two identical descriptions do not disambiguate them (the descriptions are the same), so the only real disambiguator is time.

**Ope's view:** Including description in the fingerprint doesn't fix this — identical merchant purchases have identical descriptions. The real fix is to ensure every transaction has a full datetime, not just a date, end to end.

**What's needed:**
1. **Importer side:** When a bank CSV provides only a date, the importer must synthesize a distinct time-of-day for each row rather than defaulting every row to `T00:00:00`. Simplest approach: use the row's position within the file for that date as a seconds offset (e.g. first row of 2026-04-11 → `T00:00:00`, second → `T00:00:01`, etc.), or use a higher-resolution fractional seconds field. This is per-bank importer logic: if the source has real times (OFX, some Revolut exports), use them; if not, generate a stable per-row offset.
2. **Schema/convention:** Treat `date` as always a full `YYYY-MM-DDTHH:MM:SS` and reject / upgrade any code path that creates `T00:00:00`-padded rows. Update the CLAUDE.md convention line accordingly ("Date-only imports use `T00:00:00`") — that line is exactly what's causing the collision.
3. **Fingerprint stays the same:** Once every row has a distinct datetime, the existing `sha256(datetime | amount | account_id)` hash is sufficient.

**Acceptance Criteria:**
- [ ] Importing a CSV with two same-amount transactions on the same date produces two distinct rows with distinct fingerprints
- [ ] Re-importing the same CSV is still idempotent (deterministic offset generation, not random)
- [ ] Bank CSVs that do provide time-of-day continue to use the real time

**Priority:** Medium (silent data loss; real today for any Monzo/Revolut/Lloyds user).

### B.7 Per-account monthly snapshots endpoint

**Concern:** The original handover asked for `GET /api/portfolio/snapshots?start=&end=` returning raw `{ snapshot_date, account_id, balance, currency }` rows — one per account per month. The backend shipped `GET /api/portfolio/balances?summary=true` instead, which returns only the first and last balance per account in the range (start, end, delta). That covers the accounts-grid "+£320 this period" use case but not any view that wants a monthly trend *per account*.

**Ope's view:** Low priority until a specific view needs it. Noting so it's not forgotten.

**Priority:** Low (no current caller).
