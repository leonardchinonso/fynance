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

### 1.3 Budget: standing vs per-month (OPEN QUESTION RESOLVED)

| | Backend Plan | Frontend |
|---|---|---|
| Schema | Per-month: `UNIQUE(month, category)` | Per-month: `{ month, category, amount }` |
| Behavior | Open question in docs | Generates standing targets replicated per-month |

**Context**: The backend plan flagged this as an open question. The frontend mock data takes standing budget targets and replicates them for every month. This is effectively "standing budgets implemented as per-month rows."

**Recommendation**: Keep per-month in the schema (allows seasonal variation like higher December food budget), but add a `POST /api/budget/standing` endpoint that sets a budget for all future months. The frontend's current mock behavior (same targets every month) works well with per-month storage. No model change needed.

### 1.4 Account types: property and mortgage

| | Backend Plan | Frontend |
|---|---|---|
| AccountType | `checking, savings, investment, credit, cash, pension` | Same set, but uses `savings` for property and `credit` for mortgage |

**Context**: The frontend mocks a home value as `type: "savings"` (id: "home-value") and a mortgage as `type: "credit"` (id: "mortgage-alex"). The project notes flag "property" and "mortgage"/"liability" as potential new account types.

**Recommendation**: For MVP, the frontend's approach works. The mortgage-as-credit is slightly awkward (credit cards and mortgages behave differently), but adding `property` and `mortgage` types is a post-MVP concern. The frontend already handles credit balances as liabilities via the `AVAILABLE_TYPES` classification. **No change needed for MVP.** If the backend later adds `property` and `mortgage` types, the frontend AccountType union type just needs two new members.

### 1.5 Transaction model: fully aligned

The frontend `Transaction` type matches the backend plan exactly. All fields present, same types, same semantics. No changes needed.

### 1.6 PortfolioSnapshot model: fully aligned

The frontend `PortfolioSnapshot` matches the backend schema. No changes needed.

### 1.7 HoldingType: `cash` variant removed

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
