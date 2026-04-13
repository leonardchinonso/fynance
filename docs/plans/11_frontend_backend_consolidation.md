# Frontend-Backend Consolidation Plan

This plan integrates the requirements from `docs/frontend-backend-handover.md` into the existing backend implementation plan (`09_backend_implementation_plan.md`). It restructures Phases 3-6 to incorporate all model changes, new endpoints, modified endpoints, and the delegation of frontend computation to the backend.

Phases 1 and 2 are already complete and require no changes.

> **How to use this document**: This replaces Phases 3-6 of `09_backend_implementation_plan.md`. Work through each phase top-to-bottom. The phase numbering continues from the original plan (Phase 3 onward). Each phase references the handover document section where the requirement originated.

---

## Decisions on Open Questions

Before diving into phases, here are the resolutions for the open questions raised in the handover document (Section 5). These decisions shape the implementation below.

### Q1: Profile storage: DB table (Option A)

Use a database table, not a config file. Profiles are a first-class concept that accounts reference via `profile_ids`. Storing them in the DB keeps the data model self-contained and avoids config-file-to-DB synchronization issues. The table is tiny (2-5 rows) so there is no performance concern.

### Q2: Section classification: DB table with defaults

Use a `section_mappings` table seeded with sensible defaults on first startup. The spending grid endpoint reads this mapping when classifying rows. Expose `GET /api/sections` and `PUT /api/sections` for reading and updating. This lets users customize which categories belong to which section (e.g., "Gym is a Bill, not Spending") without touching config files.

### Q3: Budget model: standing targets with per-month overrides (Option C)

**Flagged concern**: Option C adds a second table and COALESCE query logic. For MVP, the current per-month schema (Option A) works and the frontend does not care about the storage model, only the API response. However, Option C has a real UX advantage: the user sets a budget once and it applies to all months. Without it, every new month starts with an empty budget unless we build a "copy from last month" mechanism, which is effectively Option C but less clean.

**Resolution**: Proceed with Option C. Rename the existing `budgets` table to `standing_budgets` (one row per category, no month column) and add a `budget_overrides` table (month + category + amount). The backend resolves the effective budget for any month as: `COALESCE(override.amount, standing.amount)`. This keeps the common case simple (set once, applies everywhere) and supports seasonal variation (override December food budget).

**Schema changes**:
```sql
-- Replace the existing budgets table
CREATE TABLE IF NOT EXISTS standing_budgets (
    id       INTEGER PRIMARY KEY AUTOINCREMENT,
    category TEXT NOT NULL UNIQUE,
    amount   TEXT NOT NULL  -- Decimal string
);

CREATE TABLE IF NOT EXISTS budget_overrides (
    id       INTEGER PRIMARY KEY AUTOINCREMENT,
    month    TEXT NOT NULL,       -- YYYY-MM
    category TEXT NOT NULL,
    amount   TEXT NOT NULL,       -- Decimal string
    UNIQUE(month, category)
);
```

### Q4: Granularity aggregation strategy: confirmed

- Spending/cash flow endpoints: **SUM** across months in the period.
- Portfolio/balance endpoints: **LAST VALUE** (point-in-time) in the period.

This is standard and correct.

### Q5: Historical portfolio queries via `as_of`: confirmed

Already in the Phase 4 plan. The `GET /api/portfolio?as_of=YYYY-MM-DD` parameter uses carry-forward semantics. When omitted, defaults to today.

### Naming note: `by_sector` in portfolio response

The handover document calls the third breakdown "by_sector" with values like "Stocks, Pension, Cash, Other". These are really asset class groupings, not industry sectors (Technology, Healthcare, etc.). For clarity, this plan uses **`by_asset_class`** instead of `by_sector`. The classification maps account types and holding types to asset classes:
- `Stocks`: holdings with `holding_type IN ('stock', 'etf', 'fund')`
- `Bonds`: holdings with `holding_type = 'bond'`
- `Crypto`: holdings with `holding_type = 'crypto'`
- `Pension`: accounts with `type = 'pension'`
- `Cash`: accounts with `type IN ('checking', 'savings', 'cash')` plus holdings with `holding_type = 'cash'`
- `Credit`: accounts with `type = 'credit'`

If the frontend already uses `by_sector`, adjust the key name in the response serialization. The underlying data is the same.

---

## Validation & Error Handling

All error responses use the shape `{ error: String, code: String }` where `code` is machine-readable (e.g., `invalid_date_range`, `not_found`, `unauthorized`) and `error` is human-readable. This section defines what can go wrong per endpoint so handlers can be implemented without ambiguity.

### General rules

- **Date parameters** (`start`, `end`, `as_of`, `date`, `month`): reject with 400 `invalid_date` if not valid `YYYY-MM-DD` (or `YYYY-MM` where noted). Reject with 400 `invalid_date_range` if `start > end`.
- **Pagination** (`page`, `limit`): reject with 400 `invalid_pagination` if `page < 1`, `limit < 1`, or `limit > 200`.
- **Unknown filter values** (e.g., `accounts=nonexistent-id` or `categories=FakeCategory`): silently ignore. Return fewer results, not an error. The frontend only sends values it received from the server, so unknown values indicate stale UI state, not a bug.
- **Empty results**: return an empty array `[]` or a zeroed-out response object, never 404. 404 is reserved for single-resource lookups (`/transactions/:id`, `/accounts/:id`).
- **Auth**: `POST /api/import*` and `POST /api/holdings/:account_id` require a valid bearer token. Missing or invalid token returns 401 `unauthorized`. All other endpoints are accessible without auth (loopback binding is the boundary).
- **Decimal validation**: any field documented as a Decimal string must parse as a valid `rust_decimal::Decimal`. Reject with 400 `invalid_decimal` otherwise.

### Phase 3 endpoint errors

| Endpoint | Validations | Error codes |
|---|---|---|
| `GET /api/transactions` | `start`/`end` date format and range; `page`/`limit` bounds; `profile_id` if given but not found returns empty results (not 404) | `invalid_date`, `invalid_date_range`, `invalid_pagination` |
| `GET /api/transactions/by-category` | Same date/filter validation as above, no pagination | `invalid_date`, `invalid_date_range` |
| `GET /api/transactions/categories` | None (no params) | -- |
| `PATCH /api/transactions/:id` | 404 if ID not found; 400 if body is empty (no fields); 400 if `category` is empty string; ignore `category_source` if client sends it | `not_found`, `empty_body`, `invalid_category` |
| `POST /api/import` | 401 if no/invalid token; 400 if `account_id` not found; 400 if `transactions` array is empty. **Partial success**: insert valid rows, skip bad ones. Response: `ImportResult { inserted, duplicates, errors: [{ index, reason }] }` | `unauthorized`, `account_not_found`, `empty_transactions` |
| `POST /api/import/csv` | 401; 400 if no file, empty file, unparseable CSV, or missing/unknown `account` param | `unauthorized`, `missing_file`, `invalid_csv`, `account_not_found` |
| `POST /api/import/bulk` | 401; per-file errors (one file failing does not abort others). Each file gets its own `ImportResult` | Same as `/csv` per file |
| `GET /api/budget/:month` | `month` must match `YYYY-MM` | `invalid_month` |
| `GET /api/budget/spending-grid` | `start`/`end` required and validated; `granularity` must be `monthly\|quarterly\|yearly` | `invalid_date`, `invalid_date_range`, `missing_parameter`, `invalid_granularity` |
| `POST /api/budget` | 400 if `category` empty, `amount` not valid decimal, or `amount` negative. Upsert: existing standing budget for that category is overwritten | `invalid_category`, `invalid_decimal`, `negative_amount` |
| `POST /api/budget/override` | 400 if `month` not `YYYY-MM`, `category` empty, `amount` invalid/negative. Upsert on `(month, category)` | `invalid_month`, `invalid_category`, `invalid_decimal`, `negative_amount` |
| `POST /api/profiles` | 400 if `id` is empty, contains whitespace, or is not slug-like (lowercase alphanumeric + hyphens); 409 if already exists | `invalid_profile_id`, `profile_exists` |
| `GET /api/profiles` | None | -- |
| `PUT /api/sections` | 400 if any mapping has empty `section` or `category`; 400 if `section` not in `{Income, Bills, Spending, Irregular, Transfers}`. Full replacement: empty array clears all mappings (valid but logged as warning) | `invalid_section`, `invalid_category` |
| `GET /api/sections` | None | -- |
| `POST /api/accounts` | 400 if `id` empty; 409 if `id` already exists; 400 if `type` not a valid `AccountType`; 400 if `balance` provided without `balance_date` or vice versa; `profile_ids` defaults to `["default"]` | `invalid_account_id`, `account_exists`, `invalid_account_type`, `missing_balance_date` |
| `GET /api/accounts` | `profile_id` optional; unknown profile returns empty list | -- |

### Phase 4 endpoint errors

| Endpoint | Validations | Error codes |
|---|---|---|
| `GET /api/portfolio` | `as_of` if provided must be valid date; future dates clamped to today (not rejected). No accounts = valid response with all zeros | `invalid_date` |
| `GET /api/portfolio/history` | `start`/`end` required; `granularity` required and validated | `invalid_date`, `invalid_date_range`, `missing_parameter`, `invalid_granularity` |
| `GET /api/portfolio/snapshots` | `start`/`end` required; `summary` is boolean, defaults to false | `invalid_date`, `invalid_date_range`, `missing_parameter` |
| `GET /api/cash-flow` | `start`/`end` required; `granularity` validated | `invalid_date`, `invalid_date_range`, `missing_parameter`, `invalid_granularity` |
| `GET /api/holdings` | Must provide at least one of `account_id`, `account_ids`, `profile_id`; 400 if none. Unknown IDs in `account_ids` silently ignored | `missing_parameter` |
| `PATCH /api/accounts/:id/balance` | 404 if account not found; 400 if `balance` not valid decimal; 400 if `date` not valid or in the future | `not_found`, `invalid_decimal`, `invalid_date`, `future_date` |
| `POST /api/holdings/:account_id` | 401 if no/invalid token; 404 if account not found | `unauthorized`, `not_found` |

### Phase 5 endpoint errors

| Endpoint | Validations | Error codes |
|---|---|---|
| `GET /api/reports/:month` | `month` must match `YYYY-MM`; returns zeroed report if no data (not 404) | `invalid_month` |
| `GET /api/export` | Must provide `year` (or `month` for md format); `format` must be `csv` or `md`; 400 if year not `YYYY` | `missing_parameter`, `invalid_format`, `invalid_year` |

---

## Carry-Forward Semantics

Carry-forward is the core query pattern for portfolio data. It determines what balance to show for an account when no snapshot exists on the exact requested date. These rules apply uniformly to `portfolio_snapshots` and `holdings`.

### Snapshot selection rule

For any query asking "what is account X's balance as of date D":

```sql
SELECT balance, snapshot_date
FROM portfolio_snapshots
WHERE account_id = ? AND snapshot_date <= ?
ORDER BY snapshot_date DESC
LIMIT 1
```

Pick the **most recent snapshot on or before the target date**. This is already documented in `docs/design/03_data_model.md` (decision #5) and remains the canonical rule.

### No-snapshot case

If no snapshot exists at or before the target date (e.g., the account was created after the query date, or a balance was never set), the account contributes **NULL** to net worth, not zero. The API still includes the account in the response with `balance: null` so the frontend can show "Balance not set" rather than silently omitting the account or showing a misleading zero.

### Staleness: indefinite carry, surfaced indicator

Carry-forward has no expiry. An account whose last snapshot is 6 months old still contributes that balance to net worth. Rationale: pension accounts might update quarterly, property values annually. Imposing a cutoff would cause net worth to drop suddenly when a threshold is crossed, which is worse than showing a stale value.

Instead, staleness is surfaced to the user. The `PortfolioRow` returned by `Db::get_portfolio_as_of` includes an `is_stale` boolean. The threshold is **45 days**: if `snapshot_date` is more than 45 days before the `as_of` date, `is_stale = true`. The frontend renders the carried-forward date ("as of Jan 2026") and a warning indicator for stale accounts.

### Holdings carry-forward

Same selection rule: for `GET /api/holdings`, pick the latest `as_of <= target_date` **per account** (not per symbol). The query returns all holdings from the most recent snapshot date for each account:

```sql
SELECT * FROM holdings h
WHERE h.as_of = (
    SELECT MAX(h2.as_of) FROM holdings h2
    WHERE h2.account_id = h.account_id AND h2.as_of <= ?
)
```

This correctly handles sold holdings: if a symbol appears in the January snapshot but not the March snapshot, a query for April returns the March snapshot (without the sold symbol). Holdings are snapshot-as-a-whole per account, not carried forward individually per symbol.

### Portfolio history generation

`GET /api/portfolio/history` generates one data point per period by running the carry-forward query for every active account at each period end-date. The same balance can appear in consecutive months if the user hasn't updated it. This is correct behavior.

For quarterly/yearly granularity, the point-in-time is the **last day of the period** (e.g., March 31 for Q1, December 31 for yearly). The carry-forward query runs against that date.

### Account created mid-range

If a user creates an account on March 15 and the portfolio history query covers Jan-Jun, January and February show `NULL` for that account (no snapshot exists before March). The account's balance first appears in the March data point. This avoids an artificial jump in net worth. The carry-forward query handles this naturally since no snapshots exist before the account's first balance update.

### Investment metrics and carry-forward

`Db::compute_investment_metrics` uses carry-forward for the start and end values:
- **Start value**: sum of investment account balances at `start` date (carry-forward)
- **End value**: sum of investment account balances at `end` date (carry-forward)
- **New cash invested**: `SUM(amount)` from transactions with `category = 'Finance: Investment Transfer'` in the date range
- **Market growth**: `end_value - start_value - new_cash_invested`

If an investment account has no snapshot at or before the start date, it contributes `NULL` to start value. `NULL` values are excluded from the sum (not treated as zero), and market growth for that account is not computed (since the baseline is unknown).

---

## Profile Semantics

Profiles represent people in a multi-person household (e.g., Alex and Sam sharing one fynance instance). Accounts reference profiles via the `profile_ids` JSON array column.

### Account-profile association

- An account with `profile_ids: ["alex"]` belongs to Alex only.
- An account with `profile_ids: ["alex", "sam"]` is a joint account, visible to both.
- The frontend derives `is_joint` from `profile_ids.length > 1` for UI grouping (separate "Joint" section in the accounts grid). No separate `is_joint` column needed in the DB.

### Empty `profile_ids`: auto-assign to default

Accounts with empty `profile_ids` (`[]`) are auto-assigned to `["default"]`. This eliminates a special-case "empty means all" query path and keeps filtering uniform.

- **Migration**: `UPDATE accounts SET profile_ids = '["default"]' WHERE profile_ids = '[]'` (run during the Phase 3 schema migration, after the `profile_ids` column is added with `DEFAULT '[]'`)
- **`POST /api/accounts`**: if `profile_ids` is omitted or empty in the request body, set to `["default"]`
- **Seed**: the `profiles` table is seeded with `{ id: "default", name: "Default" }` if no profiles exist. This runs on first startup alongside the `section_mappings` seed.

### Filtering behavior

- **`?profile_id=alex`**: returns data for all accounts where `profile_ids` contains `"alex"`. SQL: `WHERE profile_ids LIKE '%"alex"%'`. Joint accounts appear for every profile that owns them.
- **No `profile_id` param (omitted)**: returns data for all accounts, unfiltered. This is the "household view."
- **Unknown `profile_id`**: returns empty results (no accounts match), not 404. Same behavior as other filter params.

### Joint accounts and net worth

When filtering by profile, a joint account's **full balance** is included in that profile's net worth. If Alex and Sam share a savings account with GBP 10,000, both Alex's and Sam's portfolio views show the full GBP 10,000. This is intentional: the app tracks "wealth accessible to this person," not ownership shares. Fractional attribution is post-MVP complexity.

When viewing without a profile filter (household view), joint accounts appear **once**. The backend deduplicates accounts in the unfiltered query (which it does naturally, since each account is one row). Net worth in the household view counts each account exactly once.

### Profile deletion: deferred

Profile deletion is not supported in MVP. The `GET /api/profiles` and `POST /api/profiles` endpoints are sufficient. If renaming is needed, add `PATCH /api/profiles/:id` (update `name` only, `id` is immutable).

If delete is added post-MVP, the cleanup path is: remove the deleted ID from all `accounts.profile_ids` arrays, then reassign any accounts whose `profile_ids` becomes empty to `["default"]`.

### Profile not required for queries

All endpoints accept `profile_id` as optional. When omitted, they return unfiltered data across all profiles. The frontend defaults to no profile filter and the user narrows via the profile selector dropdown.

---

## Phase 3: Profiles, Transactions API, Budget API

**Goal**: Real transaction data visible in the browser. Users can filter transactions with date ranges, multi-select filters, and free-text search. Budget spreadsheet view works with server-side aggregation. Profile support enables multi-person households.

**Reference**: Handover Sections 1.1, 2.1, 2.2, 2.3, 2.7, 3.1-3.3, 4.1, 4.5, 4.7, 4.8, 4.9

### 3.1 Schema changes

- [x] Add `profiles` table:
  ```sql
  CREATE TABLE IF NOT EXISTS profiles (
      id   TEXT PRIMARY KEY,
      name TEXT NOT NULL
  );
  ```
- [x] Add `profile_ids` column to `accounts`:
  ```sql
  ALTER TABLE accounts ADD COLUMN profile_ids TEXT NOT NULL DEFAULT '[]';
  -- JSON array of profile IDs, e.g. '["alex","sam"]'
  ```
- [x] Add `section_mappings` table:
  ```sql
  CREATE TABLE IF NOT EXISTS section_mappings (
      section  TEXT NOT NULL,  -- 'Income','Bills','Spending','Irregular','Transfers'
      category TEXT NOT NULL UNIQUE
  );
  ```
- [x] Replace `budgets` table with `standing_budgets` and `budget_overrides` (see Q3 above)
- [x] Add `short_name` column to `holdings`:
  ```sql
  ALTER TABLE holdings ADD COLUMN short_name TEXT;
  ```
- [x] Seed `section_mappings` with defaults on first startup:
  - `Income`: categories starting with `Income`
  - `Bills`: `Housing: *`, `Finance: Insurance`, `Entertainment: Streaming`
  - `Transfers`: `Finance: Savings`, `Finance: Investment`
  - `Irregular`: `Travel: *`
  - `Spending`: all remaining categories from `categories.yaml`
- [x] Seed a default profile (e.g., `{ id: "default", name: "Default" }`) if no profiles exist
- [x] Write a migration path: since Phase 1-2 are deployed, the schema changes must be additive (`ALTER TABLE ... ADD COLUMN`, new tables with `IF NOT EXISTS`). The old `budgets` table data should be migrated to `standing_budgets` where possible (group by category, take the modal amount as the standing target, put any deviations into `budget_overrides`).
- [x] After adding `profile_ids` column with `DEFAULT '[]'`, backfill existing accounts: `UPDATE accounts SET profile_ids = '["default"]' WHERE profile_ids = '[]'` (see "Profile Semantics" section above)

### 3.2 Storage additions

- [x] `Db::create_profile(&self, id: &str, name: &str) -> Result<()>`
- [x] `Db::get_profiles(&self) -> Result<Vec<Profile>>`
- [x] `Db::get_accounts(&self, profile_id: Option<&str>) -> Result<Vec<Account>>` -- filters by profile_id membership in the JSON array if provided
- [x] `Db::get_transactions(&self, filters: TransactionFilters) -> Result<(Vec<Transaction>, u64)>`:
  - `TransactionFilters` expanded: `start: Option<NaiveDate>`, `end: Option<NaiveDate>`, `accounts: Option<Vec<String>>`, `categories: Option<Vec<String>>`, `search: Option<String>`, `profile_id: Option<String>`, `page: u32`, `limit: u32`
  - Date range filtering: `WHERE date >= ? AND date <= ?`
  - Multi-select: `WHERE account_id IN (?, ?, ?)` and `WHERE category IN (?, ?, ?)`
  - Search: `WHERE (normalized LIKE ? OR description LIKE ? OR category LIKE ? OR notes LIKE ?)` with `%search_term%`
  - Profile filter: join accounts on `profile_ids` JSON contains check
  - Returns `(rows, total_count)` for pagination
- [x] `Db::get_transactions_by_category(&self, filters: TransactionFilters) -> Result<Vec<CategoryTotal>>`:
  - Same filters as above but returns `GROUP BY category` with `SUM(amount)` instead of individual rows
  - Response type: `Vec<{ category: String, total: Decimal }>`
  - Powers the bar/pie charts without downloading raw transactions
- [x] `Db::get_all_categories(&self) -> Result<Vec<String>>`:
  - Union of categories from `categories.yaml` taxonomy and `SELECT DISTINCT category FROM transactions`
  - Returns the full list so filter dropdowns show all possible categories
- [x] `Db::get_section_mappings(&self) -> Result<Vec<SectionMapping>>`
- [x] `Db::update_section_mappings(&self, mappings: &[SectionMapping]) -> Result<()>`
- [x] `Db::get_standing_budgets(&self) -> Result<Vec<StandingBudget>>`
- [x] `Db::set_standing_budget(&self, category: &str, amount: Decimal) -> Result<()>`
- [x] `Db::set_budget_override(&self, month: &str, category: &str, amount: Decimal) -> Result<()>`
- [x] `Db::get_effective_budget(&self, month: &str) -> Result<Vec<BudgetRow>>`:
  - Resolves standing + overrides: `COALESCE(override.amount, standing.amount) AS budgeted`
  - Joins with actual spending: `LEFT JOIN (SELECT category, SUM(ABS(amount)) ... FROM transactions WHERE month = ? AND amount < 0 GROUP BY category)`
  - Returns `{ category, budgeted, actual, percent }`
- [x] `Db::get_spending_grid(&self, start: NaiveDate, end: NaiveDate, granularity: Granularity, profile_id: Option<&str>) -> Result<Vec<SpendingGridRow>>`:
  - Core SQL: `GROUP BY category, substr(date, 1, 7)` for monthly, with further aggregation for quarterly/yearly
  - Joins `section_mappings` for section classification
  - Joins `standing_budgets` + `budget_overrides` for budget column
  - Computes average and total per category
  - Returns `SpendingGridRow { category, section, periods: HashMap<String, Option<Decimal>>, average, budget, total }`
- [x] `Db::insert_transactions_bulk(&self, txns: &[Transaction]) -> Result<ImportResult>` -- batch insert with dedup for API import

### 3.3 New types

- [x] `Profile { id: String, name: String }` with `serde` and `ts_rs` derives
- [x] `Granularity` enum: `Monthly | Quarterly | Yearly`, parseable from query string
- [x] `SpendingGridRow { category: String, section: String, periods: HashMap<String, Option<Decimal>>, average: Option<Decimal>, budget: Option<Decimal>, total: Option<Decimal> }`
- [x] `BudgetRow { category: String, budgeted: Option<Decimal>, actual: Decimal, percent: Option<f64> }`
- [x] `CategoryTotal { category: String, total: Decimal }`
- [x] `SectionMapping { section: String, category: String }`
- [x] `StandingBudget { category: String, amount: Decimal }`
- [x] `BudgetOverride { month: String, category: String, amount: Decimal }`
- [x] Update `Account` to include `profile_ids: Vec<String>`
- [x] Update `Holding` to include `short_name: Option<String>`

### 3.4 Transactions routes

- [x] `GET /api/transactions` (MODIFIED from original plan):
  - Query params: `start`, `end`, `accounts` (comma-separated), `categories` (comma-separated), `search`, `profile_id`, `page` (default 1), `limit` (default 25)
  - Response: `{ data: Transaction[], total: u64, page: u32, limit: u32 }`
- [x] `GET /api/transactions/by-category` (NEW, handover 4.8):
  - Same filter params as above (minus page/limit)
  - Response: `CategoryTotal[]`
  - Powers bar/pie charts without downloading raw transactions
- [x] `GET /api/transactions/categories` (MODIFIED, handover 4.9):
  - Returns full category taxonomy from `categories.yaml`, not just categories with data
  - Response: `string[]`
- [x] `PATCH /api/transactions/:id` (unchanged from original plan):
  - Body: `{ category?: string, notes?: string }`
  - Sets `category_source = 'manual'` when category is changed
  - Response: updated `Transaction`

### 3.5 Import routes

- [x] `POST /api/import` (unchanged from original plan):
  - Body: `{ account_id: string, transactions: ImportTransaction[] }`
  - Auth required (API token)
  - Dedup via fingerprint
  - Response: `ImportResult`
- [x] `POST /api/import/csv` (unchanged):
  - Multipart form: `file` + `account` (account_id)
  - Auth required
  - Response: `ImportResult`
- [x] `POST /api/import/bulk` (unchanged):
  - Multipart form: `files[]` + `accounts[]`
  - Auth required
  - Response: `ImportResult[]`

### 3.6 Budget routes

- [x] `GET /api/budget/:month` (MODIFIED, handover 4.5):
  - Returns pre-computed `BudgetRow[]` with actual spending joined against effective budget (standing + overrides)
  - Response: `[{ category, budgeted, actual, percent }]`
- [x] `GET /api/budget/spending-grid` (NEW, handover 4.1 -- CRITICAL):
  - Query params: `start`, `end`, `granularity` (monthly|quarterly|yearly), `profile_id`
  - Response: `SpendingGridRow[]`
  - The backend does the pivot, join, and aggregation that the frontend currently does in ~80 lines of JS
- [x] `POST /api/budget` (MODIFIED for Option C):
  - Body: `{ category: string, amount: string }` -- sets standing budget
  - Response: `{ ok: true }`
- [x] `POST /api/budget/override` (NEW for Option C):
  - Body: `{ month: string, category: string, amount: string }` -- sets per-month override
  - Response: `{ ok: true }`

### 3.7 Profile and section routes

- [x] `GET /api/profiles` (NEW, handover 2.1):
  - Response: `Profile[]`
- [x] `POST /api/profiles` (NEW):
  - Body: `{ id: string, name: string }`
  - Response: created `Profile`
- [x] `GET /api/sections` (NEW, handover Q2):
  - Returns all section mappings
  - Response: `SectionMapping[]`
- [x] `PUT /api/sections` (NEW):
  - Body: `SectionMapping[]` -- full replacement
  - Response: `{ ok: true }`

### 3.8 Account routes (expanded)

- [x] `GET /api/accounts` (MODIFIED, replaces `GET /api/transactions/accounts`):
  - Query params: `profile_id` (optional)
  - Response: `Account[]` with `profile_ids` field
- [x] `POST /api/accounts` (unchanged from original plan):
  - Body: `{ id, name, institution, type, currency?, balance?, balance_date?, profile_ids?, notes? }`
  - Response: `Account`

### 3.9 Ingestion checklist routes (unchanged from original plan)

- [x] `GET /api/ingestion/checklist/:month`
- [x] `POST /api/ingestion/checklist/:month/:account_id`

### 3.10 Frontend wiring

- [ ] Wire `client.ts` transaction functions to real API endpoints
- [ ] Wire `client.ts` budget functions to real API endpoints
- [ ] Replace mock spending grid computation with `GET /api/budget/spending-grid` call
- [ ] Replace mock chart aggregation with `GET /api/transactions/by-category` call
- [ ] Replace mock category list with `GET /api/transactions/categories` call
- [ ] Add profile selector component (dropdown in navbar or settings)
- [ ] Wire profile filtering through all API calls

### Deliverable

Full transaction list in the browser with date range filtering, multi-select account/category filters, free-text search, and pagination. Bar and pie charts render from server-side aggregation (no raw transaction download). Budget spreadsheet view renders from `spending-grid` endpoint with collapsible sections. Budget progress bars show pre-computed actual vs budgeted with color coding. Profile switcher works.

---

## Phase 4: Portfolio API + Portfolio UI

**Goal**: Net worth overview with available/unavailable wealth split, pre-computed breakdowns, carry-forward semantics, stock-level holdings via batch query, cash flow endpoint, and account snapshot deltas.

**Reference**: Handover Sections 2.4, 2.5, 2.6, 3.4, 4.2, 4.3, 4.4, 4.6, 4.10, 4.11

### 4.1 Portfolio routes

- [x] `GET /api/portfolio` (MODIFIED, handover 4.2):
  - Query params: `profile_id`, `as_of` (YYYY-MM-DD, defaults to today)
  - The backend computes everything server-side in a small number of queries:
    1. Fetch all active accounts (filtered by profile_id if given)
    2. For each account, get carry-forward balance as of `as_of` date
    3. Classify accounts as asset vs liability, available vs unavailable
    4. Group by type, institution, and asset class; compute sums and percentages
    5. Compute investment metrics (start/end values, new cash invested, market growth)
  - Response: `PortfolioResponse`:
    ```
    {
      net_worth: Decimal,
      currency: String,
      as_of: String,
      total_assets: Decimal,
      total_liabilities: Decimal,
      available_wealth: Decimal,    // checking + savings + investment + cash + credit
      unavailable_wealth: Decimal,  // pension (+ property future)
      accounts: Account[],          // with balance, staleness indicator
      by_type: BreakdownItem[],     // { label, value, percentage }
      by_institution: BreakdownItem[],
      by_asset_class: BreakdownItem[],
      investment_metrics: InvestmentMetrics  // { start_value, end_value, total_growth, new_cash_invested, market_growth }
    }
    ```
  - Available wealth classification: `type IN ('checking', 'savings', 'investment', 'cash', 'credit')`
  - Unavailable wealth classification: `type IN ('pension')` (extend with `property` post-MVP)

- [x] `GET /api/portfolio/history` (MODIFIED, handover 4.3):
  - Query params: `start`, `end`, `granularity` (monthly|quarterly|yearly)
  - Returns pre-aggregated rows with available/unavailable split
  - Uses **LAST VALUE** aggregation for quarterly/yearly (point-in-time, not sum)
  - Response: `PortfolioHistoryRow[]`:
    ```
    { month: String, available_wealth: Decimal, unavailable_wealth: Decimal, total_wealth: Decimal }
    ```

- [x] `GET /api/portfolio/snapshots` (NEW, handover 4.10):
  - Query params: `start`, `end`, `summary` (boolean, default false)
  - When `summary=true`: returns `{ account_id, start_balance, end_balance, delta }` per account (just first and last snapshot in range)
  - When `summary=false`: returns all `PortfolioSnapshot[]` in range
  - Powers the account grid delta indicators without downloading all monthly snapshots

- [x] `GET /api/cash-flow` (NEW, handover 4.4):
  - Query params: `start`, `end`, `profile_id`, `granularity`
  - SQL: `SELECT substr(date,1,7) AS month, SUM(CASE WHEN amount > 0 THEN amount ELSE 0 END) AS income, SUM(CASE WHEN amount < 0 THEN ABS(amount) ELSE 0 END) AS spending FROM transactions WHERE date BETWEEN ? AND ? GROUP BY month`
  - Uses SUM aggregation for quarterly/yearly
  - Response: `CashFlowMonth[]`: `{ month, income, spending }`

- [x] `PATCH /api/accounts/:id/balance` (unchanged from original plan):
  - Body: `{ balance: string, date: string }`
  - Updates `accounts.balance` and `accounts.balance_date`
  - Inserts/updates a `portfolio_snapshots` row
  - Response: updated `Account`

### 4.2 Holdings routes (MODIFIED, handover 4.6)

- [x] `GET /api/holdings` (MODIFIED to support batch query):
  - Query params: `account_id` (single), `account_ids` (comma-separated), or `profile_id`
  - When `profile_id` is given: finds all investment/pension accounts for that profile, returns all holdings grouped by account
  - Response: `Holding[]` (each includes `account_id` so the caller can group)
  - Solves the N+1 query problem where the frontend currently calls this in a loop

- [x] `POST /api/holdings/:account_id` (unchanged):
  - Body: `Holding[]` -- full replacement
  - Auth required
  - Response: `{ ok: true, holdings_updated: u32 }`

### 4.3 Storage additions

- [x] `Db::get_portfolio_as_of(&self, as_of: NaiveDate, profile_id: Option<&str>) -> Result<Vec<PortfolioRow>>`:
  - Carry-forward query (already exists from Phase 1, extend with profile filter)
  - Each row includes `{ account_id, name, institution, type, balance, as_of_date, is_stale }`

- [x] `Db::get_monthly_net_worth(&self, from: NaiveDate, to: NaiveDate, granularity: Granularity) -> Result<Vec<PortfolioHistoryRow>>`:
  - Generates a point per period using carry-forward
  - Classifies each account as available/unavailable for the split
  - For quarterly/yearly granularity, uses LAST VALUE (end-of-period balance)

- [x] `Db::get_snapshot_summary(&self, start: NaiveDate, end: NaiveDate) -> Result<Vec<SnapshotDelta>>`:
  - Returns first and last snapshot per account in range
  - `SnapshotDelta { account_id, start_balance, end_balance, delta }`

- [x] `Db::get_cash_flow(&self, start: NaiveDate, end: NaiveDate, profile_id: Option<&str>, granularity: Granularity) -> Result<Vec<CashFlowMonth>>`:
  - GROUP BY month with SUM for income (positive) and spending (negative)
  - Further aggregation for quarterly/yearly

- [x] `Db::get_holdings_batch(&self, account_ids: &[String]) -> Result<Vec<Holding>>`:
  - `SELECT * FROM holdings WHERE account_id IN (...) AND as_of = (SELECT MAX(as_of) FROM holdings WHERE account_id = h.account_id)`
  - Returns latest holdings for all requested accounts in one query

- [x] `Db::compute_investment_metrics(&self, start: NaiveDate, end: NaiveDate, profile_id: Option<&str>) -> Result<InvestmentMetrics>`:
  - Start value: sum of investment account balances at `start` (carry-forward)
  - End value: sum of investment account balances at `end` (carry-forward)
  - New cash invested: `SUM(amount) FROM transactions WHERE category = 'Finance: Investment Transfer' AND date BETWEEN start AND end`
  - Market growth: `end_value - start_value - new_cash_invested`

### 4.4 New types

- [x] `PortfolioResponse { net_worth, currency, as_of, total_assets, total_liabilities, available_wealth, unavailable_wealth, accounts, by_type, by_institution, by_asset_class, investment_metrics }`
- [x] `BreakdownItem { label: String, value: Decimal, percentage: f64 }`
- [x] `PortfolioHistoryRow { month: String, available_wealth: Decimal, unavailable_wealth: Decimal, total_wealth: Decimal }`
- [x] `CashFlowMonth { month: String, income: Decimal, spending: Decimal }`
- [x] `SnapshotDelta { account_id: String, start_balance: Option<Decimal>, end_balance: Option<Decimal>, delta: Option<Decimal> }`
- [x] `InvestmentMetrics { start_value: Decimal, end_value: Decimal, total_growth: Decimal, new_cash_invested: Decimal, market_growth: Decimal }`

### 4.5 Frontend wiring

- [ ] Wire `client.ts` portfolio functions to real API endpoints
- [ ] Replace mock portfolio computation with `GET /api/portfolio` call
- [ ] Replace mock portfolio history aggregation with `GET /api/portfolio/history` call
- [ ] Replace mock cash flow computation with `GET /api/cash-flow` call
- [ ] Replace N+1 holdings calls with batch `GET /api/holdings?profile_id=` call
- [ ] Replace mock snapshot delta computation with `GET /api/portfolio/snapshots?summary=true` call
- [ ] Wire profile and date range filters through all portfolio API calls

### Deliverable

Portfolio tab shows net worth with available/unavailable split, all account balances with staleness indicators, stock-level holdings for investment accounts (loaded in one batch call), allocation charts by type/institution/asset class, cash flow bar chart, and account balance deltas. All computed server-side. Point-in-time carry-forward works correctly. Quarterly and yearly granularity aggregation works.

---

## Phase 5: Reports + Export

**Goal**: Exportable monthly summaries, data-driven reports, the `fynance monthly` composite command, and Obsidian-compatible markdown export.

This phase is largely unchanged from the original plan (`09_backend_implementation_plan.md` Phase 5) with minor additions.

### 5.1 Reports route

- [ ] `GET /api/reports/:month`:
  - Computes a data-driven summary (no AI narrative for MVP)
  - Response: `MonthlyReport`:
    - `total_spending`, `total_income`, `net`
    - `spending_by_category: [{ category, amount, budget, pct_of_budget }]` sorted descending
    - `top_merchants: [{ name, amount, count }]` top 10
    - `net_worth_snapshot`: carry-forward portfolio total as of month-end
    - `budget_status: 'under' | 'on_track' | 'over'`
    - `month_over_month: { spending_delta_pct, income_delta_pct }` vs previous month
  - Uses the effective budget (standing + overrides) for budget comparison

### 5.2 Export routes

- [ ] `GET /api/export?year=YYYY&format=csv`:
  - Returns all transactions for the year as a CSV file with `Content-Disposition: attachment`
  - Columns: `date, description, normalized, amount, currency, account, category, notes`
- [ ] `GET /api/export?month=YYYY-MM&format=md`:
  - Obsidian-compatible markdown monthly summary
  - Response: markdown text with `Content-Type: text/markdown`
- [ ] `GET /api/export?year=YYYY&format=md`:
  - Yearly summary in markdown

### 5.3 CLI composite command

- [ ] `fynance monthly` -- orchestrates import + balance prompts
- [ ] `--dry-run` flag on `fynance import`

### 5.4 Frontend: Reports page

- [ ] Wire `client.ts` reports functions to real API
- [ ] Summary cards, category breakdown, top merchants, month-over-month deltas
- [ ] Export button (CSV, markdown)

### Deliverable

Reports tab shows a full monthly breakdown. Export endpoints produce downloadable CSV and markdown files. `fynance monthly` runs end-to-end ingestion.

---

## Phase 6: Polish + Docker + CI

**Goal**: Production readiness. Error handling, logging, Docker deployment, CI pipeline.

This phase is unchanged from the original plan (`09_backend_implementation_plan.md` Phase 6). Key items:

### 6.1 Error handling and resilience

- [x] All `AppError` variants return structured JSON `{ error, code }`
- [x] Import routes return partial success with per-row errors
- [x] All multi-insert operations use DB transactions

### 6.2 Configuration

- [x] `.env` via `dotenvy`, `categories.yaml` embedded via `include_str!`
- [x] Optional user config at `~/.config/fynance/config.yaml` (mode `0o600`)

### 6.3 Logging

- [x] `tracing-subscriber` with `FYNANCE_LOG_LEVEL`
- [x] HTTP request logging via `tower_http::trace::TraceLayer`
- [x] Never log raw transaction descriptions at `info` level

### 6.4 Docker

- [ ] Multi-stage `Dockerfile` (node build, rust build, debian-slim runtime)
- [ ] `docker-compose.yml` with volume mount for SQLite

### 6.5 CI/CD

- [ ] `ci.yml`: fmt, clippy, test, frontend build + typecheck
- [ ] `docker.yml`: build and push to GHCR

### 6.6 Final verification checklist

- [ ] `fynance serve` opens browser, all four tabs render real data
- [x] CSV import works for Monzo, Revolut, Lloyds
- [x] Deduplication prevents double-imports
- [x] API token auth blocks unauthenticated programmatic requests
- [ ] Profile filtering works across all endpoints
- [ ] Budget spreadsheet renders server-computed spending grid
- [ ] Portfolio shows net worth with available/unavailable split and carry-forward
- [ ] Holdings load in a single batch call, no N+1
- [ ] Cash flow chart renders from server-computed data
- [ ] Reports tab generates correct monthly summaries
- [ ] Export endpoints return correct CSV and markdown
- [ ] Docker container starts cleanly
- [ ] CI passes all checks

### Deliverable

The project is ready for regular use. Docker image published to GHCR. All frontend computation delegated to backend.

---

## Phase Summary

| Phase | Goal | Key Changes vs Original Plan |
|---|---|---|
| 3 | Profiles + Transactions + Budget API | Profiles table, date range filters, multi-select, search, spending-grid endpoint, by-category aggregation, standing budgets with overrides, section mappings |
| 4 | Portfolio API + carry-forward + holdings | Available/unavailable split, by_asset_class breakdown, investment metrics, batch holdings query, cash-flow endpoint, snapshot deltas, granularity support |
| 5 | Reports + Export | Minor: uses effective budget (standing + overrides) for budget comparison |
| 6 | Polish + Docker + CI | Minor: profile filtering in verification checklist |

## Endpoints Reference (complete)

### New endpoints (not in original plan)
| Endpoint | Priority | Handover Ref |
|---|---|---|
| `GET /api/profiles` | Phase 3 | 2.1, 3.1 |
| `POST /api/profiles` | Phase 3 | 2.1 |
| `GET /api/budget/spending-grid` | Phase 3, CRITICAL | 2.2, 4.1 |
| `POST /api/budget/override` | Phase 3 | Q3 |
| `GET /api/transactions/by-category` | Phase 3, MEDIUM | 4.8 |
| `GET /api/sections` | Phase 3 | Q2 |
| `PUT /api/sections` | Phase 3 | Q2 |
| `GET /api/cash-flow` | Phase 4, HIGH | 2.6, 4.4 |
| `GET /api/portfolio/snapshots` | Phase 4, LOW | 4.10 |

### Modified endpoints
| Endpoint | Changes | Handover Ref |
|---|---|---|
| `GET /api/transactions` | Date range, multi-select, search, profile_id | 3.2 |
| `GET /api/transactions/categories` | Returns full taxonomy, not just in-use | 4.9 |
| `GET /api/accounts` | Accepts profile_id, returns profile_ids field | 3.4 |
| `GET /api/budget/:month` | Pre-computed actual + effective budget | 4.5 |
| `POST /api/budget` | Sets standing budget (no month) | Q3 |
| `GET /api/portfolio` | Available/unavailable split, by_asset_class, investment_metrics | 4.2 |
| `GET /api/portfolio/history` | Available/unavailable split, granularity | 4.3 |
| `GET /api/holdings` | Batch query via account_ids or profile_id | 4.6 |

### Removed endpoints
| Endpoint | Reason | Handover Ref |
|---|---|---|
| `GET /api/transactions/accounts` | Redundant with `GET /api/accounts` | 3.2 |
| `GET /api/income/:month` | Frontend derives from transactions; cash-flow endpoint covers this | 3.3 |

### Unchanged endpoints
| Endpoint | Phase |
|---|---|
| `POST /api/import` | 3 |
| `POST /api/import/csv` | 3 |
| `POST /api/import/bulk` | 3 |
| `PATCH /api/transactions/:id` | 3 |
| `POST /api/accounts` | 3 |
| `PATCH /api/accounts/:id/balance` | 4 |
| `POST /api/holdings/:account_id` | 4 |
| `GET /api/ingestion/checklist/:month` | 3 |
| `POST /api/ingestion/checklist/:month/:account_id` | 3 |
| `GET /api/reports/:month` | 5 |
| `GET /api/export` | 5 |
| `GET /api/docs` | 2 (done) |
| Token management CLI | 2 (done) |
