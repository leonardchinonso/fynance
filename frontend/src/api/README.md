# fynance API Contract

This document defines the API contract between the React frontend and the Rust backend. The frontend currently uses `MockApiService` which returns realistic mock data. When the backend is ready, implement `RealApiService` against these endpoints and swap it in `client.ts`.

## Endpoints

### Profiles

#### `GET /api/profiles`
Returns all profiles (named users).

**Response**: `Profile[]`
```json
[
  { "id": "ope", "name": "Opemipo" },
  { "id": "tomi", "name": "Tomi" }
]
```

### Transactions

#### `GET /api/transactions`
Paginated, filterable transaction list.

**Query params**:
- `start` (string, YYYY-MM-DD) -- start of date range
- `end` (string, YYYY-MM-DD) -- end of date range
- `accounts` (string, comma-separated) -- account IDs to include
- `categories` (string, comma-separated) -- category strings to include
- `profile_id` (string) -- filter to accounts owned by this profile
- `page` (number, default 1) -- page number
- `limit` (number, default 25) -- items per page

**Response**: `PaginatedResponse<Transaction>`
```json
{
  "data": [
    {
      "id": "00000001-0000-4000-a000-000000010000",
      "date": "2026-03-15",
      "description": "LIDL GB LONDON",
      "normalized": "Lidl",
      "amount": "-42.50",
      "currency": "GBP",
      "account_id": "monzo-current",
      "category": "Food: Groceries",
      "category_source": "rule",
      "confidence": null,
      "notes": null,
      "is_recurring": false,
      "fingerprint": "a1b2c3...",
      "fitid": null
    }
  ],
  "total": 342,
  "page": 1,
  "limit": 25
}
```

#### `GET /api/transactions/categories`
Distinct categories from all transactions.

**Response**: `string[]`

#### `GET /api/accounts`
All accounts, optionally filtered by profile.

**Query params**: `profile_id` (string, optional)

**Response**: `Account[]`

### Budget

#### `GET /api/budget/:month`
Budget vs actual for a specific month (YYYY-MM).

**Response**: `BudgetRow[]`
```json
[
  {
    "category": "Food: Groceries",
    "budgeted": "300.00",
    "actual": "278.42",
    "percent": 93
  }
]
```

#### `POST /api/budget`
Set or update a budget amount.

**Body**: `BudgetUpdateRequest`
```json
{
  "month": "2026-03",
  "category": "Food: Groceries",
  "amount": "350.00"
}
```

#### `GET /api/spending-grid`
Spending grid with monthly/quarterly/yearly columns.

**Query params**: `start`, `end`, `granularity` ("monthly"|"quarterly"|"yearly")

**Response**: `SpendingGridRow[]`

### Portfolio

#### `GET /api/portfolio`
Full portfolio snapshot.

**Query params**: `profile_id` (string, optional -- omit for all profiles)

**Response**: `PortfolioResponse`
```json
{
  "net_worth": "622236.72",
  "currency": "GBP",
  "as_of": "2026-03-20",
  "total_assets": "622236.72",
  "total_liabilities": "0.00",
  "available_wealth": "357524.67",
  "unavailable_wealth": "264712.05",
  "accounts": [...],
  "by_type": [
    { "label": "investment", "total": "229386.32", "percent": 37 }
  ],
  "by_institution": [
    { "label": "Trading 212", "total": "229386.32", "percent": 37 }
  ],
  "by_sector": [
    { "label": "Stocks", "total": "229386.32", "percent": 37 }
  ]
}
```

#### `GET /api/portfolio/history`
Monthly net worth history with available/unavailable split.

**Query params**: `start`, `end` (YYYY-MM-DD, optional)

**Response**: `PortfolioHistoryRow[]`

#### `GET /api/holdings/:account_id`
Holdings for an investment account.

**Response**: `Holding[]`

#### `GET /api/cash-flow`
Monthly income vs spending.

**Query params**: `start`, `end` (YYYY-MM-DD, optional)

**Response**: `CashFlowMonth[]`

### Export

#### `GET /api/export`
Export data in CSV, image, or markdown format.

**Query params**: `format` ("csv"|"image"|"md")

---

## Deferred Features and Model Implications

These features are not implemented yet but have implications for the data model:

### RSU / Stock-Denominated Income
Vested RSUs are income valued in shares, not currency. Two approaches:
- (a) Transaction with category "Income: RSU Vesting" + a `unit` field (currency vs shares)
- (b) Separate `stock_income` table

The Transaction model may need a `unit` field or the system may need a parallel table. For now, RSUs are tracked as holdings snapshots only.

### Liability Account Types
Mortgage balance and credit card balance as negative-balance accounts. `AccountType` may need `mortgage` or a general `liability` type. Home equity = home value holding - mortgage account - HTB loan account.

### HoldingType Expansion
May need `property` for home value tracking.

### Tax Calculations for RSU Forecasting
Employer NI rate, tax rate, NI charge applied to gross RSU vesting to project net shares/value. Future `forecast` endpoint.

---

## When to Use Recharts Directly

The frontend uses Tremor for all charts (built on Recharts, Apache 2.0). If these advanced chart types are needed in the future, drop to raw Recharts (same engine, no new dependency):

- **Click-to-filter**: clicking a pie slice to filter a table view
- **Synchronized cursors**: hovering one chart highlights the same data point on another
- **Custom animated transitions**: chart morphing between view modes
- **Candlestick / OHLC charts**: stock price visualization
- **Waterfall charts**: income-to-savings flow visualization
- **Sankey diagrams**: money flow between accounts
- **Brush/zoom on charts**: drag to select a time range on a chart to zoom in
