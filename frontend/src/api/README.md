# fynance API Contract

This document defines the API contract between the React frontend and the Rust backend. The frontend uses `MockApiService` which returns realistic mock data. When the backend is ready, implement `RealApiService` against these endpoints and swap it in `client.ts`.

All services are defined in `service.ts` as the `ApiService` interface. Each method corresponds to a backend endpoint.

---

## Service Methods

### `getProfiles(): Promise<Profile[]>`

**Backend endpoint**: `GET /api/profiles`

Returns all named profiles (users). Each profile has accounts associated with it.

**Response**: `Profile[]`
```json
[{ "id": "alex", "name": "Alex" }, { "id": "sam", "name": "Sam" }]
```

**Backend notes**: Profiles are a lightweight concept. Could be a simple table or config file. Accounts reference profiles via `profile_ids` (array, for joint accounts).

---

### `getTransactions(filters: TransactionFilters): Promise<PaginatedResponse<Transaction>>`

**Backend endpoint**: `GET /api/transactions`

Paginated, filterable transaction list. This is the primary data query for the Transactions page. **This must be a server-side query** -- the frontend does NOT load all transactions and filter client-side. The backend should apply all filters and return only the requested page.

**Query params**:
| Param | Type | Description |
|---|---|---|
| `start` | string (YYYY-MM-DD) | Start of date range |
| `end` | string (YYYY-MM-DD) | End of date range |
| `accounts` | string (comma-separated) | Account IDs to include |
| `categories` | string (comma-separated) | Category strings to include |
| `search` | string | Free-text search across `normalized`, `description`, `category`, `account_id`, `notes` |
| `profile_id` | string | Filter to accounts owned by this profile |
| `page` | number (default 1) | Page number |
| `limit` | number (default 25) | Items per page (user-configurable: 10, 25, 50, 100) |

**Response**: `PaginatedResponse<Transaction>`
```json
{
  "data": [{ "id": "...", "date": "2026-03-15", "normalized": "Lidl", "amount": "-42.50", ... }],
  "total": 1988,
  "page": 1,
  "limit": 25
}
```

**Backend notes**: The `search` param should do case-insensitive substring matching across all text fields. Consider using SQLite FTS5 for performance at scale. The frontend sends the search as a URL param and expects the backend to filter server-side.

---

### `getCategories(): Promise<string[]>`

**Backend endpoint**: `GET /api/transactions/categories`

Returns distinct category strings from all transactions.

**Backend notes**: Simple `SELECT DISTINCT category FROM transactions WHERE category IS NOT NULL ORDER BY category`.

---

### `getAccounts(profileId?: string): Promise<Account[]>`

**Backend endpoint**: `GET /api/accounts?profile_id=<id>`

Returns all accounts, optionally filtered by profile. Joint accounts (with multiple `profile_ids`) should appear when filtering to ANY of their owners.

**Backend notes**: `Account.profile_ids` is an array. If `profile_id` is provided, return accounts where `profile_ids` contains that ID (not equality, membership check). This supports joint accounts.

---

### `getBudget(month: string): Promise<BudgetRow[]>`

**Backend endpoint**: `GET /api/budget/:month`

Budget vs actual spending for a specific month (YYYY-MM). Joins budget targets with actual transaction sums per category.

**Response**: `BudgetRow[]`
```json
[{ "category": "Food: Groceries", "budgeted": "300.00", "actual": "278.42", "percent": 93 }]
```

**Backend notes**: `actual` is computed by summing `ABS(amount)` for negative transactions in the given month grouped by category. `percent` = `actual / budgeted * 100`.

---

### `getSpendingGrid(start, end, granularity, profileId?): Promise<SpendingGridRow[]>`

**Backend endpoint**: `GET /api/spending-grid?start=&end=&granularity=&profile_id=`

Monthly/quarterly/yearly spending grid for the budget spreadsheet view. Returns one row per category with values for each time period.

**Response**: `SpendingGridRow[]`
```json
[{
  "category": "Food: Groceries",
  "section": "Spending",
  "months": { "2025-10": "-278.42", "2025-11": "-312.50", "2026-04": null },
  "average": "-295.46",
  "budget": "300.00",
  "total": "-590.92"
}]
```

**Key behavior**:
- `months` values are `null` for months with no data (not "0"). The frontend shows "-" for null months and omits them from charts.
- `section` determines grouping: "Income", "Bills", "Spending", "Irregular", "Transfers"
- `average` is computed only from months with data
- If `profile_id` is provided, only include transactions from accounts owned by that profile

**Backend notes**: The `granularity` param can be "monthly", "quarterly", or "yearly". The frontend currently aggregates quarterly/yearly values client-side from monthly data, but ideally the backend would return pre-aggregated data for performance. Decision: start with monthly data + client aggregation, optimize later if needed.

---

### `updateBudget(req: BudgetUpdateRequest): Promise<void>`

**Backend endpoint**: `POST /api/budget`

Set or update a budget amount for a category + month.

**Body**: `{ "month": "2026-03", "category": "Food: Groceries", "amount": "350.00" }`

---

### `getPortfolio(profileId?): Promise<PortfolioResponse>`

**Backend endpoint**: `GET /api/portfolio?profile_id=<id>`

Full portfolio snapshot with breakdowns. This is the primary data query for the Portfolio Overview page.

**Response**: `PortfolioResponse` with `net_worth`, `available_wealth`, `unavailable_wealth`, `accounts`, `by_type`, `by_institution`, `by_sector`.

**Backend notes**:
- `available_wealth` = sum of checking + savings + investment + cash balances
- `unavailable_wealth` = sum of pension balances
- `by_type`, `by_institution`, `by_sector` are computed aggregations with percentages
- Joint accounts should be included when filtering to either owner

---

### `getPortfolioHistory(start?, end?): Promise<PortfolioHistoryRow[]>`

**Backend endpoint**: `GET /api/portfolio/history?start=&end=`

Monthly net worth history with available/unavailable split. Used for the portfolio history line chart and table.

**Response**: `PortfolioHistoryRow[]`
```json
[{ "month": "2025-04", "available_wealth": "71833.28", "unavailable_wealth": "76200.23", "total_wealth": "148033.51" }]
```

**Backend notes**: Aggregate `portfolio_snapshots` by month. For each month, sum account balances split by available/unavailable type. The frontend handles quarterly/yearly aggregation client-side.

---

### `getAccountSnapshots(start?, end?): Promise<PortfolioSnapshot[]>`

**Backend endpoint**: `GET /api/portfolio/snapshots?start=&end=`

Raw per-account monthly balance snapshots. Used by the accounts grid to compute per-card deltas (change from start of selected period to current balance).

**Response**: `PortfolioSnapshot[]`
```json
[{ "snapshot_date": "2025-04-01", "account_id": "monzo-current", "balance": "2800.00", "currency": "GBP" }]
```

---

### `getHoldings(accountId): Promise<Holding[]>`

**Backend endpoint**: `GET /api/holdings/:account_id`

Holdings for an investment account. Used in the holdings drill-down sheet.

---

### `getCashFlow(start?, end?): Promise<CashFlowMonth[]>`

**Backend endpoint**: `GET /api/cash-flow?start=&end=`

Monthly income vs spending. Derived from transactions.

---

### `exportData(format): Promise<void>`

**Backend endpoint**: `GET /api/export?format=csv|image|md`

Not yet implemented. Currently shows a "coming soon" toast.

---

## Data Model Notes

### Joint Accounts

`Account.profile_ids` is an array of profile IDs. A joint account has multiple owners (e.g., `["alex", "sam"]`). When filtering by profile, include accounts where `profile_ids` contains the selected profile. In the accounts grid, joint accounts are displayed in a separate "Joint Accounts" section.

### Null vs Zero in Spending Grid

Months with no transaction data return `null` in the spending grid, NOT `"0"`. This distinction is critical:
- `null` = no data recorded yet (shows as "-" in spreadsheet, excluded from charts)
- `"0"` = data was recorded but the amount was zero

### Available vs Unavailable Wealth

- **Available**: checking, savings, investment, cash, credit
- **Unavailable**: pension (and future: property/home equity)

---

## Deferred Features and Model Implications

### RSU / Stock-Denominated Income
Vested RSUs are income valued in shares, not currency. The Transaction model may need a `unit` field (currency vs shares) or a parallel `stock_income` table.

### Liability Account Types
Mortgage balance and credit card balance as negative-balance accounts. `AccountType` may need `mortgage` or `liability`. `HoldingType` may need `property`.

### Tax Calculations for RSU Forecasting
Employer NI rate, tax rate applied to gross RSU vesting. Future `forecast` endpoint.

---

## When to Use Recharts Directly

The frontend uses Tremor for some chart styling but primarily uses Recharts directly. For these advanced interactions, use raw Recharts:
- Click-to-filter: clicking a pie slice to filter the table
- Synchronized cursors across charts
- Candlestick/OHLC charts for stock prices
- Waterfall charts for income-to-savings flow
- Sankey diagrams for money flow
- Brush/zoom (already implemented on portfolio history)
