# Multi-Currency Support

Centralized spec for all multi-currency work across backend and frontend. Referenced from `19_v0_burndown.md` (V0 items) and `20_post_v0_plans.md` (V2+ items).

---

## Mental Model

All amounts are stored in their **source currency** at ingestion — never converted on write. Conversion happens at query time on the backend when aggregating across accounts.

The app maintains a single global list of currencies. The user declares:
1. Which currencies are in use in the app (the **currencies table**)
2. Which one is the **preferred currency** (the basis for all aggregations)
3. For every non-preferred currency, the **exchange rate to the preferred currency** — stored as a mandatory field on the currency row, not separately

Any sum, total, or aggregation that mixes currencies first converts each value to the preferred currency using the stored rate, then adds them up. There is no other way to combine values across currencies.

**Invariants that must always hold:**
- There is always exactly one preferred currency
- Every currency in the table has a non-null, positive `fx_rate`
- Every holding, account, and transaction currency must exist in the currencies table — creating one with an unsupported currency is a hard error
- A currency that is referenced by at least one holding, account, or transaction cannot be deleted

---

## User Setup Flow (V0)

GBP is seeded automatically at app initialisation and set as the preferred currency. The user does not need to do anything for a GBP-only portfolio. For multi-currency portfolios, they add currencies and rates in Settings.

### Add a currency

- UI: "Add currency" button in the Currencies section of Settings — opens a picker (searchable by ISO 4217 code or name)
- When adding a currency, the exchange rate input is shown immediately in the same dialog — the user must enter it before saving. Rate cannot be saved as zero or blank.
- Label format: `1 [SOURCE] = ___ [PREFERRED]`. Example: `1 USD = 0.79 GBP`

### Set preferred currency

- UI: a star icon next to each currency in the list. Filled star = preferred. Clicking a different currency's star transfers the preferred status to it.
- The preferred currency row is read-only: rate shows `1.00`, rate field is disabled, delete button is disabled (tooltip: "Set a different preferred currency first").
- Tooltip/callout: "Your preferred currency is the basis for all calculations. Any amount in a different currency is converted to your preferred currency before being included in totals, budgets, or charts."

### Staleness indicator

Each non-preferred currency row shows when its rate was last updated: "Last updated X days ago" in muted text. If older than 30 days, shown as an amber warning. This is a frontend-only display for V0 — in V2 the backend will use this timestamp to auto-refresh stale rates.

---

## Backend (V0)

### Data model

#### `currencies` table (new, app-level)

One row per currency in use. This is global — not scoped to a profile or user. `code` is the primary key (ISO 4217 codes are unique).

```sql
CREATE TABLE IF NOT EXISTS currencies (
    code            TEXT PRIMARY KEY,                -- ISO 4217, e.g. 'GBP', 'NGN', 'USD'
    is_preferred    INTEGER NOT NULL DEFAULT 0,      -- 1 for exactly one row, 0 for all others
    fx_rate         TEXT NOT NULL,                   -- Decimal string. 1 unit of code = fx_rate units of preferred. Always '1' for the preferred row.
    updated_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);
```

Constraints enforced at the application layer (SQLite has no CHECK across rows):
- Exactly one row has `is_preferred = 1` at all times
- `fx_rate` must be a positive Decimal; zero is rejected at the API layer
- The preferred row always has `fx_rate = '1'`

Seeded at startup if the table is empty: insert `('GBP', 1, '1', now())`.

Rate convention: `fx_rate` is always expressed as **1 unit of `current_base_currency` = `fx_rate` units of `preferred_currency`**. So if preferred is GBP and base is NGN, a rate of `0.00051` means 1 NGN = 0.00051 GBP. The frontend must make this direction clear in its label.

`updated_at` is surfaced in the API response and shown in the frontend as a staleness indicator.

#### No changes to `profile` table

Currency config is app-level. `preferred_currency` and `supported_currencies` are **not** added to the profile.

#### `exchange_rates` table — V2 only

Not created in V0. Reserved for the V2 auto-fetch cache. See V2 section.

### API

All currency operations go through a single `/api/currencies` endpoint family — no separate `/api/fx-rates`.

```
GET    /api/currencies                  -- list all currencies with is_preferred, fx_rate, updated_at
POST   /api/currencies                  -- add a currency (code + fx_rate required)
PATCH  /api/currencies/:code            -- update fx_rate or transfer preferred status
DELETE /api/currencies/:code            -- delete (rejected if currency is in use)
```

**POST `/api/currencies`** — body:
```json
{ "code": "NGN", "fx_rate": "0.00051" }
```
Rejected if `code` already exists, if `fx_rate` is zero or missing, or if `code` is not a valid ISO 4217 code.

**PATCH `/api/currencies/:code`** — body (all fields optional, at least one required):
```json
{ "fx_rate": "0.00049", "is_preferred": true }
```
Setting `is_preferred: true` on a currency atomically clears `is_preferred` on the current preferred row and updates the new preferred's `fx_rate` to `"1"`. Setting `fx_rate` on the preferred row is rejected.

**DELETE `/api/currencies/:code`** — rejected with a 409 if the currency is referenced by any holding, account, or transaction. Rejected with a 400 if it is the preferred currency.

**GET `/api/currencies`** — response:
```json
[
  { "code": "GBP", "is_preferred": true,  "fx_rate": "1",       "updated_at": null },
  { "code": "NGN", "is_preferred": false, "fx_rate": "0.00051", "updated_at": "2026-01-15T10:00:00Z" },
  { "code": "USD", "is_preferred": false, "fx_rate": "0.79",    "updated_at": "2026-04-30T08:22:00Z" }
]
```
The preferred currency always has `fx_rate: "1"` and `updated_at: null` (it is never manually set by the user).

### Write-time currency validation

Any endpoint that creates or updates a holding, account, or transaction must validate that the supplied `currency` exists in the `currencies` table. If it does not, return a 422 with a clear error: `"Currency 'EUR' is not configured. Add it in Settings before using it."` This is enforced at the API layer before any DB write.

### Conversion logic (V0)

When computing any aggregated value:

1. Load all rows from `currencies` into a map `{ code -> fx_rate }`.
2. For each value being aggregated, look up the rate for its currency. If `currency == preferred`, rate is `1`. If the currency is not in the map, **throw** — this should never happen because write-time validation ensures all currencies are supported.
3. Multiply `value * fx_rate` to get the amount in `preferred_currency`, then sum.
4. All aggregated totals in responses are in `preferred_currency`. Every aggregating response includes a top-level `preferred_currency: String` field so the frontend always knows the denomination.

### `display_currency` — the rule

Every aggregating endpoint applies this rule uniformly:

> All numeric values in the response are always in `preferred_currency`. Alongside any aggregated value, include an optional `display_currency` object. Set it **only** when every source value that contributed to that aggregation shares the same non-preferred currency. When set, it contains the raw sum in that source currency and the currency code. When inputs are mixed-currency, or all already in `preferred_currency`, omit it.

#### The `DisplayCurrency` struct (shared across all endpoints)

```rust
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct DisplayCurrency {
    /// Raw sum in the source currency (before conversion), as a Decimal string.
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub value: Decimal,
    /// ISO 4217 currency code of the source currency, e.g. "NGN".
    pub currency: String,
}
```

---

### Per-endpoint `display_currency` spec

Conversion and `display_currency` logic is applied in Rust after fetching rows from SQLite. SQL queries do not change.

---

#### 1. `GET /api/holdings/summary` → `HoldingsSummaryResponse`

Add `preferred_currency: String` at the top level.

Top-level scalar totals (`net_worth`, `total_assets`, etc.) aggregate across all accounts and will nearly always be mixed-currency — no `display_currency` on these.

Add `display_currency: Option<DisplayCurrency>` to `BreakdownItem`:

```rust
pub struct BreakdownItem {
    pub label: String,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub value: Decimal,            // always in preferred_currency
    pub percentage: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_currency: Option<DisplayCurrency>,
}
```

Example — `by_institution`, GT Bank (all NGN):
```json
{
  "label": "GT Bank",
  "value": "80.15",
  "percentage": 4.9,
  "display_currency": { "value": "156543.00", "currency": "NGN" }
}
```

Example — `by_institution`, Monzo (GBP = preferred): no `display_currency`.

Example — `by_asset_class` Cash (mixes GBP + NGN): no `display_currency` — mixed currencies.

---

#### 2. `GET /api/holdings/history` → `Vec<HoldingsHistoryRow>`

Almost always mixed-currency. Include `display_currency` fields for correctness on single-currency portfolios.

```rust
pub struct HoldingsHistoryRow {
    pub month: String,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub available_wealth: Decimal,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available_wealth_display: Option<DisplayCurrency>,

    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub unavailable_wealth: Decimal,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable_wealth_display: Option<DisplayCurrency>,

    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub total_wealth: Decimal,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_wealth_display: Option<DisplayCurrency>,
}
```

---

#### 3. `GET /api/holdings/balances` → `Vec<AccountSnapshot>` or `Vec<BalanceDelta>`

Per-account balances — no cross-currency summing within a row. **No change to these structs.** Both already carry `currency: String`.

---

#### 4. `GET /api/holdings/cash-flow` → `Vec<HoldingsCashFlowMonth>`

```rust
pub struct HoldingsCashFlowMonth {
    pub month: String,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub income: Decimal,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub income_display: Option<DisplayCurrency>,

    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub spending: Decimal,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spending_display: Option<DisplayCurrency>,
}
```

---

#### 5. `GET /api/budget/spending-grid` → `Vec<SpendingGridRow>`

`periods` values, `average`, and `total` are all converted to `preferred_currency`. Add a parallel display map for the per-period values:

```rust
pub struct SpendingGridRow {
    pub category: String,
    pub category_id: Option<String>,
    pub section: String,
    #[ts(type = "Record<string, string | null>")]
    pub periods: HashMap<String, Option<String>>,          // preferred_currency
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    #[ts(type = "Record<string, DisplayCurrency>")]
    pub periods_display: HashMap<String, DisplayCurrency>,
    pub average: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub average_display: Option<DisplayCurrency>,
    pub total: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_display: Option<DisplayCurrency>,
}
```

---

#### 6. `GET /api/budget/:month` → `Vec<BudgetRow>`

`budgeted` is user-entered in `preferred_currency` — no display_currency needed. `actual` is the converted sum of transactions.

```rust
pub struct BudgetRow {
    pub category: String,
    pub category_id: Option<String>,
    pub budgeted: Option<String>,
    pub actual: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual_display: Option<DisplayCurrency>,
    pub percent: Option<f64>,
}
```

---

#### 7. `GET /api/transactions/by-category` → `Vec<CategoryTotal>`

```rust
pub struct CategoryTotal {
    pub category: String,
    pub total: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_currency: Option<DisplayCurrency>,
}
```

---

## Frontend (V0)

### Settings page: Currency section

New section in Settings (between Profiles and Accounts).

**Layout:**
- Header: "Currencies"
- Info callout: "Your preferred currency is the basis for all portfolio calculations. Set an exchange rate for each additional currency you hold so totals can be combined."
- List of configured currencies, each row:
  - Currency code + name (e.g. "NGN — Nigerian Naira")
  - Star icon: filled = preferred, outline = not. Clicking transfers preferred status.
  - Exchange rate: `1 [CODE] = ___ [PREFERRED]` — disabled and showing `1.00` for the preferred row
  - Staleness label: "Last updated X days/months ago" in muted text. Amber warning if older than 30 days.
  - Delete button — disabled for the preferred currency and for any currency in use (tooltip explains why)
- "Add currency" button — opens a picker + rate input dialog; both fields required before saving

**Behavior:**
- Changing the preferred currency: the old preferred row's rate is cleared (user must re-enter); the new preferred row's rate is set to `1` automatically. Prompt: "Changing your preferred currency will require you to re-enter exchange rates for all other currencies."
- All saves go to `PATCH /api/currencies/:code` or `POST /api/currencies`.
- If only GBP is configured and a multi-currency holding exists (shouldn't happen given write-time validation, but defensively): show a setup prompt.

### The frontend display rule

Applies to every component rendering an aggregated monetary value:

> **Use the numeric value for all calculations and chart sizing** — it is always in `preferred_currency`. **For labels, axis ticks, and tooltips**: show `display_currency.value` + `display_currency.currency` when the field is present; otherwise show the numeric value + `preferred_currency`.

Examples:
- Portfolio pie chart: slice size from `value` (GBP). Slice label: "GT Bank — ₦156,543". Tooltip: "₦156,543 NGN (£80.15 GBP)".
- Spending grid cell: bar width from `periods["2026-01"]` (GBP). Text from `periods_display["2026-01"]` if set, else GBP value.
- Budget row: bar width from `actual`. Label from `actual_display` if present.

---

## V2: Automatic Rate Fetching

V0 is fully user-driven. V2 adds automatic fetching to keep rates fresh.

On each holdings summary request, for each non-preferred currency:
1. Check `currencies.updated_at`. If older than the staleness threshold (default: 1 day), fetch a fresh rate from [frankfurter.app](https://frankfurter.app) — free, no API key, ECB data.
2. Write the fetched rate back to `currencies` (updating `fx_rate` and `updated_at`). Also cache in an `exchange_rates` table keyed by `(base, quote, date, source='api')` for historical lookups.
3. Rates that the user has manually pinned (`pinned: bool` field added to `currencies` in V2) are never auto-overwritten.

**Additional endpoints (V2):**
- `DELETE /api/currencies/cache` — force re-fetch on next request
- `GET /api/currencies/resolved` — active rates with source and `updated_at`
- If provider unavailable: fall back to existing rate, include `stale_rates: true` in the summary response

**Frontend (V2):** Staleness labels update automatically after each portfolio load. Amber threshold raised (e.g. warn only if provider has been unreachable >7 days).

---

## V3: User-Configurable FX Provider

- Settings: FX provider section — URL template and API key, stored on the profile.
- Backend: use user-configured provider if set, fall back to frankfurter.
- Long-term: multiple providers with priority order.

---

## V4: Historical Exchange Rates

Use `as_of` date on holdings snapshots to fetch the rate current at snapshot time, not today's, for accurate historical net worth charts.

- `exchange_rates` table (added in V2) caches by date — historical lookups query it, fetching from provider if missing.
- Holdings snapshots already capture value + currency at snapshot date — this is purely about using the right rate per date when aggregating history.

---

## Known limitation: rate-to-preferred only (V0)

Exchange rates in V0 are stored as `1 [source] = X [preferred]`. This means if the user wants to view their portfolio in a currency other than `preferred_currency`, the conversion cannot be derived from the stored data alone — they would need to change their preferred currency and re-enter all rates.

Currency pairs (e.g. storing USD↔GBP and USD↔NGN independently) would allow switching preferred currency without re-entering data, but adds schema and UX complexity. This is deferred to V2+ if the need arises.
