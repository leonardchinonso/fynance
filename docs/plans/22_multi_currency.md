# Multi-Currency Support

Centralized spec for all multi-currency work across backend and frontend. Referenced from `19_v0_burndown.md` (V0 items) and `20_post_v0_plans.md` (V2+ items).

---

## Mental Model

All amounts are stored in their **source currency** at ingestion — never converted on write. Conversion happens at query time on the backend when aggregating across accounts.

The user declares:
1. Which currencies they use in the app (their **supported currencies list**)
2. Which one is their **preferred currency** (the basis for all aggregations)
3. For every non-preferred currency, what the **exchange rate is to the preferred currency**

Any sum, total, or aggregation that mixes currencies first converts each value to the preferred currency using the stored rate, then adds them up. There is no other way to combine values across currencies.

---

## User Setup Flow (V0)

The user must complete a one-time currency setup before portfolio aggregations are meaningful. The frontend should surface this as a setup step if no supported currencies are configured.

### Step 1: Add supported currencies

The user adds every currency they hold money in. Example: GBP, USD, NGN.

- UI: a list with an "Add currency" input (ISO 4217 code or searchable name)
- No limit on how many they can add

### Step 2: Set preferred currency

The user picks one currency from their supported list as the preferred currency. GBP is seeded automatically on profile creation and set as the default, preserving the invariant that there is always exactly one preferred currency. The first currency the user manually adds (if they change it) becomes preferred; after that, they explicitly star a different one to change it.

- UI: a star icon next to each currency in the list; clicking it sets that one as preferred. The current preferred currency always has a filled star and cannot be deleted (delete is disabled with a tooltip: "Remove preferred status first").
- Tooltip/callout: "Your preferred currency is the basis for all calculations. Any amount in a different currency is converted to your preferred currency before being included in totals, budgets, or charts."
- Backend: `preferred_currency` defaults to `"GBP"` and `supported_currencies` defaults to `["GBP"]` on the profile. These are seeded at profile creation time, not lazily.

### Step 3: Set exchange rates

For every non-preferred currency the user has added, they must provide an exchange rate to the preferred currency.

- UI: an exchange rate input next to each non-preferred currency
- Label format: `1 [SOURCE] = X [PREFERRED]`. Example: `1 USD = 0.79 GBP`
- The preferred currency itself always has a rate of `1` (shown read-only, not editable)
- Validation: rate must be a positive decimal; zero is not allowed

The backend stores these rates. They are the authoritative rates used for all conversions until the user updates them.

---

## Backend (V0)

### Data model changes

#### `profile` table

Add two fields:

```sql
preferred_currency  TEXT NOT NULL DEFAULT 'GBP',
supported_currencies TEXT NOT NULL DEFAULT '["GBP"]'  -- JSON array of ISO 4217 codes
```

#### `user_fx_rates` table (new)

Stores the user's manually-set exchange rates. One row per (preferred, source) pair.

```sql
CREATE TABLE IF NOT EXISTS user_fx_rates (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    profile_id      TEXT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    base_currency   TEXT NOT NULL,   -- the source currency, e.g. 'NGN'
    quote_currency  TEXT NOT NULL,   -- the preferred currency, e.g. 'GBP'
    rate            TEXT NOT NULL,   -- Decimal (TEXT), e.g. '0.00051'. 1 base = rate quote.
    updated_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    UNIQUE(profile_id, base_currency, quote_currency)
);

CREATE INDEX IF NOT EXISTS idx_user_fx_profile ON user_fx_rates(profile_id);
```

Rate convention: `rate` is always expressed as **1 unit of base = rate units of quote**. So if preferred is GBP and base is NGN, a rate of `0.00051` means 1 NGN = 0.00051 GBP. The frontend must make this direction clear in its label.

`updated_at` is surfaced in the API response and shown in the frontend as a staleness indicator (e.g. "Last updated 3 months ago — may need updating").

#### `exchange_rates` table (existing design, from `13_frontend_backend_handover_unimplemented.md`)

This table is retained for historical rate caching (V2+). Schema already designed:

```sql
CREATE TABLE IF NOT EXISTS exchange_rates (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    base_currency   TEXT NOT NULL,
    quote_currency  TEXT NOT NULL,
    rate            TEXT NOT NULL,          -- Decimal, never float
    as_of_date      TEXT NOT NULL,
    source          TEXT NOT NULL DEFAULT 'manual',  -- 'manual', 'api', etc.
    created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    UNIQUE(base_currency, quote_currency, as_of_date, source)
);

CREATE INDEX IF NOT EXISTS idx_ex_currencies ON exchange_rates(base_currency, quote_currency);
CREATE INDEX IF NOT EXISTS idx_ex_date ON exchange_rates(as_of_date);
```

Not used in V0 aggregations — `user_fx_rates` is the sole source of truth for conversion in V0. This table is reserved for the V2 auto-fetch cache.

### API changes

#### Profile endpoints (extend existing)

```
GET  /api/profiles/:id          -- include preferred_currency and supported_currencies in response
PATCH /api/profiles/:id         -- allow updating preferred_currency and supported_currencies
```

Request body for PATCH:
```json
{
  "preferred_currency": "GBP",
  "supported_currencies": ["GBP", "USD", "NGN"]
}
```

#### FX rate endpoints (new)

```
GET  /api/fx-rates              -- list all user_fx_rates for the active profile
PUT  /api/fx-rates              -- replace all fx rates for the profile (full replace, not merge)
```

PUT body:
```json
[
  { "base_currency": "NGN", "rate": "0.00051" },
  { "base_currency": "USD", "rate": "0.79" }
]
```

`quote_currency` is always the profile's `preferred_currency` — no need to pass it. The preferred currency itself is not included (its rate is implicitly 1).

Response for GET:
```json
[
  { "base_currency": "NGN", "quote_currency": "GBP", "rate": "0.00051", "updated_at": "2026-01-15T10:00:00Z" },
  { "base_currency": "USD", "quote_currency": "GBP", "rate": "0.79",    "updated_at": "2026-04-30T08:22:00Z" },
  { "base_currency": "GBP", "quote_currency": "GBP", "rate": "1",       "updated_at": null }
]
```

The preferred currency row with `rate: "1"` and `updated_at: null` is always included in the GET response so the frontend can render it as a read-only row. The frontend uses `updated_at` to show staleness: if the rate was set more than (e.g.) 30 days ago, display a warning next to the rate — "Last updated 3 months ago — may need updating".

### Conversion logic (V0)

When computing any aggregated value (holdings summary totals, `by_type`, `by_institution`, `by_asset_class` breakdowns):

1. Load all `user_fx_rates` for the active profile into a map `{ base -> rate }`.
2. For each holding value, look up the rate for its currency. If the currency equals `preferred_currency`, rate is 1. If no rate found, **exclude the holding from the aggregation and include it in an `unconverted` list** in the response (do not silently add it as-is).
3. Multiply `value * rate` to get the converted amount, sum across all holdings.
4. All summary totals are in `preferred_currency`. The response includes `preferred_currency: "GBP"` at the top level so the frontend always knows what currency the numbers are in.

### `display_currency` — the rule

Every aggregating endpoint must apply this rule uniformly:

> All numeric values in the response are always in `preferred_currency`. Alongside any aggregated value, include an optional `display_currency` object. Set it only when **every** source value that contributed to that aggregation shares the same non-preferred currency. When set, it contains the raw sum in that source currency and the currency code. When inputs are mixed-currency or all already in `preferred_currency`, omit it.

The frontend rule that follows from this: **always use the numeric value** (which is in `preferred_currency`) **for calculations and chart sizing**. For labels, axis ticks, and tooltips: show `display_currency.value` + `display_currency.currency` if present, otherwise show the numeric value + `preferred_currency`.

#### The `DisplayCurrency` struct (shared across all endpoints)

```rust
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct DisplayCurrency {
    /// The raw sum in the source currency (before conversion), as a Decimal string.
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub value: Decimal,
    /// ISO 4217 currency code of the source currency, e.g. "NGN", "USD".
    pub currency: String,
}
```

---

### Per-endpoint `display_currency` spec

Every endpoint below already does aggregation in Rust after fetching rows from SQLite. The conversion and `display_currency` logic is added in Rust at that point — the SQL queries themselves do not change.

---

#### 1. `GET /api/holdings/summary` → `HoldingsSummaryResponse`

**Current struct fields:** `net_worth`, `total_assets`, `total_liabilities`, `available_wealth`, `unavailable_wealth`, `by_type: Vec<BreakdownItem>`, `by_institution: Vec<BreakdownItem>`, `by_asset_class: Vec<BreakdownItem>`, `accounts: Vec<Account>`, `investment_metrics`.

**Changes:**

Add `preferred_currency: String` and `unconverted: Vec<UnconvertedHolding>` at the top level (see Unconverted holdings below).

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

Example — `by_institution` with GT Bank (all NGN holdings):
```json
{
  "label": "GT Bank",
  "value": "80.15",
  "percentage": 4.9,
  "display_currency": { "value": "156543.00", "currency": "NGN" }
}
```

Example — `by_institution` with Monzo (GBP, same as preferred):
```json
{ "label": "Monzo", "value": "2934.06", "percentage": 18.1 }
```
No `display_currency` — source currency equals preferred.

Example — `by_asset_class` Cash row (mixes GBP + NGN):
```json
{ "label": "Cash", "value": "2934.06", "percentage": 18.1 }
```
No `display_currency` — mixed source currencies.

---

#### 2. `GET /api/holdings/history` → `Vec<HoldingsHistoryRow>`

**Current struct fields:** `month`, `available_wealth`, `unavailable_wealth`, `total_wealth`.

Each row is a point-in-time snapshot summing all accounts. Almost always mixed-currency, so `display_currency` will rarely be set. Include it anyway for correctness (single-currency portfolios).

```rust
pub struct HoldingsHistoryRow {
    pub month: String,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub available_wealth: Decimal,    // always preferred_currency
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available_wealth_display: Option<DisplayCurrency>,

    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub unavailable_wealth: Decimal,  // always preferred_currency
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable_wealth_display: Option<DisplayCurrency>,

    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub total_wealth: Decimal,        // always preferred_currency
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_wealth_display: Option<DisplayCurrency>,
}
```

---

#### 3. `GET /api/holdings/balances` → `Vec<AccountSnapshot>` or `Vec<BalanceDelta>`

**`AccountSnapshot`** is a per-account balance at a point in time. A single account has one currency — no aggregation across currencies within a row. The `balance` field stays as-is (source currency). No conversion needed here: this endpoint returns per-account raw balances, not cross-account totals. **No change to this struct.**

**`BalanceDelta`** is likewise per-account. Same reasoning — **no change**.

Both structs already carry `currency: String` (AccountSnapshot) or the frontend can derive it from the account. No conversion needed because there is no cross-currency summing here.

---

#### 4. `GET /api/holdings/cash-flow` → `Vec<HoldingsCashFlowMonth>`

**Current struct fields:** `month`, `income`, `spending`.

Each row sums transactions across all accounts for a time period — almost always mixed-currency. Include `display_currency` fields for the single-currency case.

```rust
pub struct HoldingsCashFlowMonth {
    pub month: String,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub income: Decimal,             // always preferred_currency
    #[serde(skip_serializing_if = "Option::is_none")]
    pub income_display: Option<DisplayCurrency>,

    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub spending: Decimal,           // always preferred_currency
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spending_display: Option<DisplayCurrency>,
}
```

---

#### 5. `GET /api/budget/spending-grid` → `Vec<SpendingGridRow>`

**Current struct fields:** `category`, `category_id`, `section`, `periods: HashMap<String, Option<String>>`, `average`, `total`.

The `periods` map values, `average`, and `total` are all aggregated amounts. They could be mixed-currency (transactions across multi-currency accounts in the same category). Include `display_currency` alongside each period value and the totals.

Because `periods` is a `HashMap<String, Option<String>>` (period key → decimal string), add a parallel map for display values:

```rust
pub struct SpendingGridRow {
    pub category: String,
    pub category_id: Option<String>,
    pub section: String,
    /// Period key -> decimal string in preferred_currency (or null).
    #[ts(type = "Record<string, string | null>")]
    pub periods: HashMap<String, Option<String>>,
    /// Period key -> DisplayCurrency (only set when all transactions in that period share one non-preferred currency).
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    #[ts(type = "Record<string, DisplayCurrency>")]
    pub periods_display: HashMap<String, DisplayCurrency>,
    pub average: Option<String>,                           // preferred_currency
    #[serde(skip_serializing_if = "Option::is_none")]
    pub average_display: Option<DisplayCurrency>,
    pub total: Option<String>,                             // preferred_currency
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_display: Option<DisplayCurrency>,
}
```

---

#### 6. `GET /api/budget/:month` → `Vec<BudgetRow>`

**Current struct fields:** `category`, `category_id`, `budgeted`, `actual`, `percent`.

`actual` is the sum of transactions in that category for the month. `budgeted` is a user-set amount — it is always in the user's preferred currency (entered by the user), so no display_currency needed on it.

```rust
pub struct BudgetRow {
    pub category: String,
    pub category_id: Option<String>,
    pub budgeted: Option<String>,                          // preferred_currency, user-entered
    pub actual: String,                                    // preferred_currency after conversion
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual_display: Option<DisplayCurrency>,
    pub percent: Option<f64>,
}
```

---

#### 7. `GET /api/transactions/by-category` → `Vec<CategoryTotal>`

**Current struct fields:** `category`, `total`.

`total` is the sum of transactions in that category. Could span multiple currencies.

```rust
pub struct CategoryTotal {
    pub category: String,
    pub total: String,                                     // preferred_currency after conversion
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_currency: Option<DisplayCurrency>,
}
```

---

### Unconverted holdings

If any holding or transaction has a currency with no configured exchange rate, the API must not silently include it in totals. Return it in a top-level `unconverted` array on any endpoint where this can occur (primarily the holdings endpoints; the transaction endpoints can do the same).

Add this struct:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct UnconvertedItem {
    pub currency: String,
    #[serde(with = "rust_decimal::serde::str")]
    #[ts(type = "string")]
    pub value: String,
    pub account_id: Option<String>,  // present for holdings, absent for transaction aggregations
}
```

`HoldingsSummaryResponse`, `HoldingsHistoryRow`-wrapper, `HoldingsCashFlowMonth`-wrapper, and the `GET /api/transactions/by-category` response should all include:

```rust
#[serde(default, skip_serializing_if = "Vec::is_empty")]
pub unconverted: Vec<UnconvertedItem>,
pub preferred_currency: String,
```

Frontend: if `unconverted` is non-empty, show a warning callout: "Some holdings/transactions could not be included in totals — no exchange rate set for [EUR, ...]. Configure in Settings."

---

## Frontend (V0)

### Settings page: Currency section

New section in the Settings page, between Profiles and Accounts (or wherever it fits logically).

**Layout:**
- Header: "Currencies"
- Info callout: "Your preferred currency is the basis for all portfolio calculations. Set an exchange rate for each additional currency you hold so totals can be combined."
- List of configured currencies, each row showing:
  - Currency code + name (e.g. "NGN — Nigerian Naira")
  - Star icon: filled if preferred, outline if not. Clicking sets this as preferred and recalculates all rates.
  - Exchange rate input: `1 [CODE] = ___ [PREFERRED]` (read-only `1.00` for the preferred currency row)
  - Staleness label: "Last updated X days/months ago" shown in muted text next to the rate. If older than 30 days, show as an amber warning.
  - Delete button (disabled for preferred currency)
- "Add currency" button opens a currency picker (searchable by code or name)

**Behavior:**
- Changing the preferred currency resets all exchange rate inputs (since the quote side changes) and prompts the user to re-enter rates.
- Saving rates calls `PUT /api/fx-rates`.
- If no rates are configured and the user has multi-currency holdings, show a banner on the Portfolio page: "Exchange rates not configured. Portfolio totals may be incomplete. Configure in Settings."

### Portfolio page and all other pages: the frontend display rule

This rule applies to every component that renders an aggregated monetary value returned by the backend:

> **Use the numeric value for all calculations and chart sizing** (it is always in `preferred_currency`). **For labels, axis ticks, and tooltips**, show `display_currency.value` + `display_currency.currency` if the field is present; otherwise show the numeric value + `preferred_currency`.

Examples:
- Portfolio pie chart: slice size is computed from `value` (GBP). The slice label reads "GT Bank — ₦156,543" (from `display_currency`). Tooltip shows "₦156,543 NGN (£80.15 GBP)".
- Spending grid cell: the bar width or number is the `periods["2026-01"]` value in GBP. The text in the cell reads the `periods_display["2026-01"]` value + currency if set, else GBP.
- Budget row: bar width from `actual` (GBP). Label text from `actual_display` if present.
- If `unconverted` is non-empty on any response, show an amber warning banner: "Some values couldn't be converted — no exchange rate set for [EUR]. Configure rates in Settings." with a link to the Settings currency section.

---

## V2: Automatic Rate Fetching

V0 is fully user-driven: the user sets rates manually and the `updated_at` timestamp tells them how old the rate is. V2 adds automatic fetching to keep rates fresh without manual intervention.

**How it works:**

On each holdings summary request, for each non-preferred currency in the portfolio:
1. Check `user_fx_rates.updated_at` for that currency. If the rate is older than the staleness threshold (configurable, default: 1 day), treat it as stale.
2. If stale (or no rate exists), fetch a fresh rate from the provider ([frankfurter.app](https://frankfurter.app) — free, no API key, ECB data).
3. Write the fetched rate back into `user_fx_rates` (updating `rate` and `updated_at`), and also cache it in `exchange_rates` keyed by `(base, quote, date, source='api')`.
4. Continue using the now-refreshed rate for the aggregation.

The user's manually-entered rates behave identically — they are stored in the same `user_fx_rates` table with the same `updated_at`. The difference is that auto-fetch overwrites stale entries automatically, whereas manual entries only update when the user saves them. If the user wants to pin a rate (e.g. NGN official rate is not representative), they can set it manually and the auto-refresh will not overwrite it — instead, only rates with `source='api'` in the `exchange_rates` cache are auto-refreshed; manually pinned rates in `user_fx_rates` are left alone. (Implementation detail: add a `pinned: bool` field to `user_fx_rates` for V2.)

**Additional endpoints (V2):**
- `DELETE /api/fx-rates/cache` — force-invalidate the auto-fetch cache, triggering re-fetch on next request
- `GET /api/fx-rates/resolved` — view active rates for the profile (manual + auto-fetched, showing source and `updated_at` for each)
- If the provider is unavailable, fall back to the most recent cached rate and include `stale_rates: true` in the holdings summary response

**Frontend (V2):** The staleness label in Settings (from V0) now updates automatically after each portfolio load. The amber warning threshold may be raised since rates auto-refresh — e.g. only warn if the provider has been unreachable for >7 days.

---

## V3: User-Configurable FX Provider

- Settings page: FX provider section — URL template and API key input, stored on the profile.
- Backend: when fetching exchange rates, use the user-configured provider if set, fall back to frankfurter.
- Long-term: allow multiple providers with priority order.

---

## V4: Historical Exchange Rates

Use `as_of` date on holdings snapshots to fetch the exchange rate that was current at snapshot time, not today's rate, for accurate historical net worth chart values.

- `exchange_rates` table already caches by date — historical lookups just query that table, fetching from the provider if the date is missing.
- Holdings snapshots already capture value + currency at snapshot date, so the raw data is correct. This is purely about using the right rate per date when aggregating history.

---

## Open Questions

- Should `supported_currencies` be stored as a JSON array on the profile, or as a separate `profile_currencies` join table? JSON array is simpler for V0; a join table gives cleaner constraints if we need per-currency metadata later. Decision: **JSON array for V0**, migrate to join table if needed in V2.
- If the user changes their preferred currency, do historical snapshot totals become meaningless? For V0 yes — warn the user that changing preferred currency will affect all historical aggregated views.
