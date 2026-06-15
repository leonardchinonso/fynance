# V0 Burndown

Everything needed to ship a usable V0. Split by owner. These items were pulled from a conversation between Ope and Nonso on 2026-04-18 and reconciled against existing design docs.

> **Re-audit 2026-06-14:** This archived doc was re-checked against the current code. Most items that were marked `⚠️` "deferred to V1" have since been implemented in later feature work (PDF/Excel parsing via `/api/parse`, document storage + provenance, transaction & CSV dry-run, periodic-holdings snapshot extraction, parse hints, budget average spend, account edit/delete, multi-currency frontend). Those were flipped to `✅`. Items re-confirmed **still open** are annotated inline; the genuine outstanding work is the small cleanup list captured at the bottom of this file under "Still open after 2026-06-14 audit".

---

## Nonso (Backend / API)

### Holdings / Portfolio

- [x] ✅ Rename portfolio endpoint to `/api/holdings` (get rid of all references to `portfolio` as it's confusing, we don't need back compat)
  - All routes renamed in `server/mod.rs` lines 82-107
  - Portfolio endpoints now under `/api/holdings` hierarchy
  - Old portfolio.rs deleted (git status shows deletion)
- [x] ✅ Implement importing holding balances from documents
  - POST `/api/holdings/import` implemented (holdings.rs:349)
  - Dry-run support: query param `?dry_run=true` returns previews (holdings.rs:346, 365-368)
  - HoldingsImportPayload struct (model.rs:431-435)
- [x] ✅ Allow multiple cash holdings per account
  - schema.sql: `sub_account` field added (line 90)
  - Unique constraint updated to include sub_account (lines 96-97): `UNIQUE(account_id, symbol, COALESCE(sub_account, ''), as_of)`
  - Holding struct includes `sub_account: Option<String>` (model.rs:410)
  - Monzo pots now fully supported
- [x] ✅ Support marking a holding as closed
  - schema.sql: `is_closed INTEGER NOT NULL DEFAULT 0` (line 91)
  - Holding struct includes `is_closed: bool` (model.rs:412)
  - Index on is_closed for query filtering (line 101)
  - Patch endpoint at `/api/holdings/:account_id/:symbol` (holdings.rs:416)

- ⚠️ **Fix holding write model: union of scalar value vs. quantity+price** — **STILL OPEN (2026-06-14 audit):** `HoldingsImportPayload` is still `{ account_id, holdings: Vec<Holding> }` and `Holding` still carries `quantity: Decimal` (non-optional), `price_per_unit: Option`, and `value: Decimal` all at once — no tagged union, no both-arms validation. — currently the import payload requires `quantity` (non-optional) and optionally `price_per_unit`, with `value` also present, meaning all three can be set simultaneously with no consistency guarantee. The request type should be a tagged union: either `{ value }` (scalar, for cash/property/loan) or `{ quantity, price_per_unit }` (computed, for stocks/ETFs/crypto), with the backend deriving `value = quantity * price_per_unit` in the computed case. The response `Holding` struct stays flat (`value` always set, `quantity`/`price_per_unit` optional) so the frontend never needs to pattern-match on reads. Validation should reject payloads that supply fields from both arms.

### Accounts

- [x] ✅ **PATCH and DELETE endpoints for accounts and profiles**
  - `PATCH /api/accounts/:id` (routes/accounts.rs) — name, institution, type, currency, is_active, profile_ids, notes; at least one field required.
  - `DELETE /api/accounts/:id` — 409 `account_in_use` if any transactions/holdings reference; soft-delete via `is_active = 0`.
  - `PATCH /api/profiles/:id` (routes/profiles.rs) — name only.
  - `DELETE /api/profiles/:id` — protects `default`; 409 `profile_in_use` if any account still references via `profile_ids` JSON array.
  - All wired in server/mod.rs.

- [x] ✅ **Expand holding PATCH + add holding DELETE**
  - `PatchHoldingRequest` now accepts optional value, currency, new_sub_account in addition to is_closed (routes/holdings.rs).
  - Scope (which row to update) is still `as_of` + existing `sub_account`; `new_sub_account` is the rename target.
  - `DELETE /api/holdings/:account_id/:symbol` (already wired via the multi-method route at server/mod.rs:133-138).
  - `update_holding_fields` added in storage/db.rs for the value/currency/sub_account writes.

- [x] ✅ **Remove `accounts.balance` and `accounts.balance_date` columns** (Option A: minimal — table columns dropped, struct fields preserved and runtime-computed from holdings; chosen 2026-05-20 to avoid touching 9 frontend usages).

- [x] ✅ Account `type` field should be an enum
  - AccountType enum defined (model.rs:110-148) with: Checking, Savings, Investment, Credit, Cash, Pension, Property, Mortgage
  - Schema: `type TEXT NOT NULL` on accounts table (line 55)
  - Account struct uses AccountType (model.rs:82)
  - Includes as_str() and parse() methods for serialization


- [x] ✅ **Multi-currency: fulfill all backend asks** — currencies table, FX conversion at query time, display_currency on all 6 aggregating endpoints. Full spec: [docs/plans/22_multi_currency.md](../22_multi_currency.md).

### Budget

- [x] ✅ Every category has a budget; auto-carry from previous month unless overridden
  - schema.sql: standing_budgets table (lines 150-154) stores per-category standing amounts
  - schema.sql: budget_overrides table (lines 159-165) stores per-month category overrides
  - Routes: POST /api/budget (budget.rs) sets standing budgets
  - Routes: POST /api/budget/override sets monthly overrides
  - GET /api/budget/:month retrieves effective budget for the month
- [x] ✅ Storage location decided
  - Stored in two separate tables: standing_budgets (per-category) and budget_overrides (per-month overrides)
  - Design allows auto-carry: query uses COALESCE(override.amount, standing.amount)

### Categories

**Model:** Hierarchical categories table (parent-child max depth 2) linked to display sections via section_mappings.

- [x] ✅ Full categories table created (schema.sql, lines 43-52)
  - id (TEXT PRIMARY KEY), name (TEXT UNIQUE), parent_id, display_order, is_active, created/updated timestamps
  - Supports hierarchical structure: parent categories for grouping, leaf nodes assignable to transactions
  - Seeded from categories.yaml on first startup
  - Routes: GET/POST/PATCH/DELETE for category CRUD (routes/categories.rs)
- [x] ✅ Categories linked to sections (section_mappings table, lines 163-172)
  - Schema maps category_id to section (Income | Bills | Spending | Irregular | Transfers)
  - Routes: PUT /api/sections replaces all section mappings (sections.rs)
  - Routes: GET /api/sections lists current mappings
- [x] ✅ Category-transaction association
  - Transaction model includes `category: Option<String>` (legacy) and `category_id: Option<String>` (FK)
  - schema.sql: category_id foreign key to categories.id (line 20, 29)
  - PATCH /api/transactions/:id allows updating category_id (transactions.rs:213)

- ⚠️ **Drop legacy `category` string field from all structs** — **STILL OPEN (2026-06-14 audit):** still present on `Transaction` (model.rs:36), `SectionMapping` (381), `StandingBudget` (391), `ImportTransaction` (421), plus the `category` field on budget.rs:100/150 and transactions.rs:191 request bodies and unified.rs:43; all still marked "kept for backward compat". — `Transaction`, `SectionMapping`, `StandingBudget`, `BudgetRow`, `SpendingGridRow`, `ImportTransaction`, `SetStandingBudgetBody`, `SetBudgetOverrideBody` all carry both `category: Option<String>` (legacy display name) and `category_id` (FK). The `category` field is explicitly marked "kept for backward compat" in model.rs comments. Once agents are updated to send `category_id`, remove `category` from all structs; ts-rs will regenerate the frontend bindings automatically and no frontend changes are needed.

### Transactions

- [x] ✅ Add an `exclude_from_summary` boolean flag on individual transactions (default false)
  - schema.sql: `exclude_from_summary INTEGER NOT NULL DEFAULT 0` (line 25)
  - model.rs: Transaction struct includes `exclude_from_summary: bool` (line 43)
  - Index on exclude_from_summary for filtering (line 37)
  - PATCH endpoint respects this field (transactions.rs:213)
  - Query filters exclude these rows in spending-grid, cash-flow, by-category (db.rs lines 613, 1075, 1100, 1181)
  - ImportTransaction payload includes field (model.rs:362)
- ⚠️ Fingerprint collision disambiguation — **STILL DEFERRED (2026-06-14 audit):** util/mod.rs `fingerprint()` is still `sha256(datetime, amount, account_id)`; no `duplicate_index`. Intentional deferral, not a regression.
  - **Status:** Deferred - using simple sha256(datetime, amount, account_id) fingerprint
  - For same-day same-amount collisions, optional `duplicate_index` could be added later
  - Current approach: rely on LLM categorization + uniqueness checks

### API: Endpoints

Implemented endpoints by entity:

| Entity | Endpoints | Status |
|---|---|---|
| Transactions | GET /api/transactions, PATCH /api/transactions/:id, GET /api/transactions/by-category, GET /api/transactions/categories, GET /api/transactions/accounts | ✅ Done |
| Holdings | GET /api/holdings, POST /api/holdings/import (?dry_run), POST /api/holdings/:account_id, PATCH /api/holdings/:account_id/:symbol, (+ summary/history/balances/cash-flow views) | ✅ Done |
| Categories | POST /api/categories, GET /api/categories, GET /api/categories/:id, GET /api/categories/resolve, PATCH /api/categories/:id, DELETE /api/categories/:id | ✅ Done (full CRUD) |
| Sections | GET /api/sections, PUT /api/sections (replaces all mappings) | ✅ Done |
| Accounts | GET /api/accounts, POST /api/accounts, PATCH /api/accounts/:id, DELETE /api/accounts/:id, PATCH /api/accounts/:id/balance | ✅ Done (full CRUD) |
| Profiles | GET /api/profiles, POST /api/profiles, PATCH /api/profiles/:id, DELETE /api/profiles/:id | ✅ Done (full CRUD) |
| Import | POST /api/import (JSON), POST /api/import/csv, POST /api/import/bulk | ✅ Done (no dry_run) |
| Budget | GET /api/budget/:month, POST /api/budget (standing), POST /api/budget/override (monthly) | ✅ Done |

Dry-run support:
- [x] ✅ Transactions: `?dry_run=true` implemented on POST /api/transactions/import and POST /api/import/csv (import_api.rs:95, 198)
  - Returns a TransactionImportPreview (new/duplicate/error row counts) without committing
  - Backed by `dry_run_transactions` / `dry_run_transactions_from_parsed` in storage/db.rs
- [x] ✅ Holdings: `?dry_run=true` query param returns previews without committing (holdings.rs:346, 365-368)
  - Returns HoldingPreview list with status field
  - Supports efficient confirmation via repeated call with dry_run=false
- [x] ✅ Every endpoint documented in OpenAPI spec (GET /api/docs)

### Document imports

CSV is supported. PDFs and images deferred to V1.

- [x] ✅ CSV uploads: POST /api/import/csv (import_api.rs:79), POST /api/import/bulk (import_api.rs:136)
- [x] ✅ PDF uploads: implemented via POST /api/parse (pdf_parser.rs; format_detection.rs validates the `%PDF` header and enforces a 20-page cap)
- ⚠️ Image/screenshot uploads: **STILL OPEN** — format_detection.rs handles CSV/Excel/PDF only; no image MIME path. Tracked in docs/plans/20_post_v0_plans.md.
- ⚠️ Multi-file per account: multi-file upload + parallel per-file extraction is done (document_parser.rs `run_multi_file_pipeline`); true **cross-file LLM context still open** (tracked in 20_post_v0_plans.md).
- [x] ✅ Optional `hints` field: ParseHints.hint surfaced verbatim to every parser prompt (document_parser.rs; pdf_parser.rs `user_hint`)
- [x] ✅ Dry-run for imports: supported in holdings import (holdings.rs:346), transaction import flow

##### Trading 212
- [x] ✅ T212 PDF parsing: covered by the bank-agnostic LLM PDF parser (pdf_parser.rs)
- [x] ✅ Opening/closing balance extraction: periodic_holdings_parser extracts period-end snapshots (document_parser.rs `pdf_periodic_holdings` / `csv_periodic_holdings`)

##### Monzo
- [x] ✅ CSV parsing: fully supported, bank-detected by LLM
- [x] ✅ PDF parsing: covered by the bank-agnostic LLM PDF parser (pdf_parser.rs)
- [x] ✅ Multiple pots support: schema supports `sub_account` field for multiple cash holdings per account
- [x] ✅ Multiple document upload: POST /api/parse accepts multiple files (`run_multi_file_pipeline`)

#### Holding Snapshots from Imports

- [x] ✅ Automatic snapshot extraction: periodic_holdings_parser extracts period-end balances from CSV/PDF (document_parser.rs `csv_periodic_holdings` / `pdf_periodic_holdings`)
- [x] ✅ Multi-holding snapshots: extraction produces a `Vec<Holding>` per period, each carrying the `derived` provenance flag

### API Documentation

- [x] ✅ `GET /api/docs` returns OpenAPI 3.1 spec (routes/docs.rs:35)
- [x] ✅ Endpoints documented with schemas and examples
- [x] ✅ Category taxonomy documentation: fully embedded in OpenAPI spec
  - Full category tree from categories.yaml embedded in response (routes/docs.rs:24, 286)
  - Includes all parent-child relationships and display order
  - Available at /api/docs under "x-categories" component

### Dry run

- [x] ✅ Holdings dry-run: `?dry_run=true` query param on POST /api/holdings/import (holdings.rs:346, 365-368)
  - Returns HoldingPreview structs with `status` field indicating create/modify/conflict
  - Does NOT write to database
  - Supports efficient confirmation via repeated call with dry_run=false
- [x] ✅ CSV import dry-run: `?dry_run=true` implemented on POST /api/import/csv (import_api.rs:198; `CsvImportQuery.dry_run`)
  - Returns row previews via `dry_run_transactions_from_parsed` without committing

### Currency

- [x] ✅ Currency tracked at transaction level
  - schema.sql: `currency TEXT NOT NULL DEFAULT 'GBP'` (line 17)
  - model.rs: Transaction struct includes `currency: String` (line 33)
  - Wired through routes and API
- [x] ✅ Currency tracked at holding level
  - schema.sql: `currency TEXT NOT NULL DEFAULT 'GBP'` on holdings (line 87)
  - model.rs: Holding struct includes `currency: String` (line 403)
  - Surfaced in all holding endpoints
- [x] ✅ Currency tracked at budget level
  - Standing budgets and overrides inherit category context (no separate currency field, assumes account currency)
  - Amounts are Decimal strings, currency implicit per account
- [x] ✅ Source currency convention
  - All amounts stored as TEXT (Decimal) in original source currency
  - No conversion at ingestion
  - Documented in CLAUDE.md and schema comments

### Type Sharing (ts-rs)

- ⚠️ Generic `Paginated<T>` struct: **Not implemented** — **STILL OPEN (2026-06-14 audit):** no `Paginated<T>` struct in backend/src.
  - Could be added in future for cleaner API responses
  - Current endpoints return either single objects or arrays
  - Would require frontend PaginatedResponse refactor (deferred)


### Type System Cleanup

- [x] ✅ **Remove `AccountType::Mortgage`:** Removed from enum, `as_str()`, `parse()`, and `account_type_to_asset_class()`. Migration 8 in `migrate_schema()` converts existing `account_type = 'mortgage'` rows to `property` on startup. `InvestmentIsa` added as a new variant in the same pass.
- [x] ✅ **Add `is_available: bool` to the `Account` response struct:** Field added to `Account` struct, populated by `is_available_account()` in all three `Account` construction sites (`row_to_account`, `accounts.rs`, `account.rs`). ts-rs will regenerate the frontend binding automatically.

---

## Ope (Frontend / Import / Data Ingestion)

### Settings Page: Remaining Work

- [x] ✅ **Consolidate Accounts and Data Ingestion.** Ingestion section merged into Accounts — drag-to-reorder and eye toggle now on each account row. Separate Data Ingestion section removed.
- [x] ✅ **Fixed sidebar navigation.** `overflow-x-hidden` removed from `main` in App.tsx — sticky now works correctly.
- [x] ✅ **Skeleton loading states.** Implemented via RemoteData system — all pages and leaf components show skeletons on load.
- [x] ✅ **Tests for profile/account creation.** Test infrastructure ready, tests not yet written
- ⚠️ **tests for CSV import.** **STILL OPEN** — no frontend unit/integration tests exist (only the Playwright smoke script). Test infrastructure ready, tests not written.
- [x] ✅ **Edit/delete buttons.** Wired to backend PATCH/DELETE — `EditAccountDialog` + delete-confirm dialog in settings/accounts_section.tsx (Pencil / Trash2 icons).
- [x] ✅ **Multi-currency frontend:** Settings currency section (currencies_section.tsx) with preferred-currency picker + star, preferred_currency_context, and `DualAmount` display using `preferred_currency` / `display_currency` across portfolio and budget. Full spec: [docs/plans/22_multi_currency.md](../22_multi_currency.md).

### Build: Fix Pre-existing TypeScript Errors

- [x] ✅ `date_range_selector.tsx`: ToggleGroup fixed
- [x] ✅ `view_mode_switcher.tsx`: ToggleGroup fixed
- [x] ✅ `budget_spreadsheet.tsx`: unused variables fixed
- [x] ✅ `transactions.tsx`: unused PieChart import fixed
- [x] ✅ `vite.config.ts`: React Compiler babel issue fixed with ts-ignore
  - Note: Remove ts-ignore once upstream fixes the type definition
- [x] ✅ undefined array access: handled with proper type guards
- [x] ✅ Type casting: removed, using type guards instead
- [x] ✅ Docker registry test: completed and working
- [x] ✅ Mock data updated for new backend fields
  - `mock_holdings.ts`: added `sub_account: null` and `is_closed: false` to all holdings
  - `mock_transactions.ts`: added `category_id: null` and `exclude_from_summary: false` to transactions
  - `mock_service.ts`: added `category_id: null` to BudgetRow and SpendingGridRow responses

### Transactions (UI)

- [x] ✅ `exclude_from_summary` flag in backend
  - Backend fully implements flag with database storage and query filtering
  - UI renders disabled switch with "Coming soon" tooltip (transactions.tsx)
  - [x] ✅ UI switch now functional — clicking toggles the flag via `PATCH /api/transactions/:id` with optimistic update

### Budget (UI)

- [x] ✅ Budget display (read-only)
  - Backend: standing_budgets + budget_overrides tables with auto-carry via COALESCE (db.rs)
  - Frontend: SpendingGridRow includes budget field, displayed in spending grid view
  - API calls exist: setStandingBudget (POST /api/budget), setBudgetOverride (POST /api/budget/override)
- [x] ✅ Budget editing UI: **Done**
  - Inline edit popover on the Budget column in the spending grid
  - Clicking the budget cell for any category opens a popover to set the standing monthly budget
  - Saves via `POST /api/budget` (standing) or `POST /api/budget/override` (monthly)
  - Grid refreshes automatically after saving
- [x] ✅ Average spend calculation: `rowAvg` / `average_display` rendered per row in budget_spreadsheet.tsx
- ⚠️ Budget tooltip on hover: **STILL OPEN** — no spending-trend hover tooltip in budget_spreadsheet.tsx
- ⚠️ Show empty categories toggle: **STILL OPEN** — not implemented in the budget grid

### Category / Category ID Cleanup

- ✅ **Frontend bindings are auto-generated from Rust via ts-rs — no frontend changes needed here.** Once the backend drops the legacy `category` field from its structs, the bindings regenerate automatically and the frontend just follows.

### Type Sharing (ts-rs)

- ⚠️ Drop hand-written `PaginatedResponse<T>`: **Deferred** — **STILL OPEN (2026-06-14 audit):** still hand-written in frontend/src/types/api.ts:55.
  - Depends on generic `Paginated<T>` struct implementation on backend
  - Current endpoints return arrays or single objects
  - Can be added in future refactor

### Completed (this PR: `feat/frontend-v0-burndown`)

- [x] **DraggableList component** extracted from navbar saved views into reusable `draggable_list.tsx`
- [x] **Settings page** created with 6 sections: Profiles, Accounts, Categories, Data Ingestion, Appearance, Data Source
- [x] **Profiles section:** list profiles, add profile dialog (create via `POST /api/profiles`)
- [x] **Accounts section:** list accounts with type badge and balance, add account dialog (create via `POST /api/accounts`)
- [x] **Categories section:** grouped list with add/edit/delete (backend CRUD endpoints available: POST/GET/PATCH/DELETE /api/categories)
- [x] **Data Ingestion section:** account ordering via DraggableList, hide/show accounts, stored in localStorage
- [x] **Appearance section:** Light/Dark/System theme toggle (moved from navbar)
  - ⚠️ **"System" mode broken on mobile** — **NEEDS MOBILE RE-TEST (2026-06-14 audit):** use_theme.ts now resolves `system` via `matchMedia("(prefers-color-scheme: dark)")` and subscribes to changes, so the code path looks correct; the original mobile-only bug can't be confirmed or refuted statically. Verify on a real device / mobile emulator.
- [x] **Data Source section:** Live/Mock toggle with MOCK_ONLY env var support (moved from navbar)
- [x] **Navbar changes:** removed theme and mock/live toggles, added Import CTA popover, added Settings gear icon
- [x] **Import wizard** (`/import?mode=wizard`): step through accounts with file upload, skip, preview results, completion summary
- [x] **Import single mode** (`/import?mode=single`): select account, upload files, view results
- [x] **File upload component** with drag-and-drop, file list, deduplicate by name+size
- [x] **Preview table** showing import stats (total, new, duplicates), bank detection, error table
- [x] **Wizard progress sidebar** with check/skip/current icons per account
- [x] **Import summary** with per-account result cards and navigation
- [x] **Ingestion preferences hook** (`use_ingestion_preferences.ts`): localStorage-based account ordering
- [x] **API service extensions:** `createProfile`, `createAccount`, category CRUD (mock), `importCsv` (multipart)
- [x] **Default API mode flipped** from mock to live, added `VITE_MOCK_ONLY` support
- [x] **Dockerfile** (multi-stage: Node frontend, Rust backend, debian-slim runtime)
- [x] **docker-compose.yml** with GHCR image and persistent volume
- [x] **GitHub Actions CI** (frontend lint+build, backend test+clippy)
- [x] **GitHub Actions Docker publish** (auto-version tagging to GHCR on push to master)
- [x] **Transaction exclude column** added to table (disabled switch with "Coming soon" tooltip)
- [x] **shadcn Switch component** added
- [x] **TypeScript errors fixed** for Base UI compatibility (render prop, ToggleGroup array API)

---

## Shared / Open Questions & Decisions

- ✅ **Rules-based fallback for categorization:** Deferred to V3. Current design relies on LLM + manual categorization.
- ✅ **T212 closing positions:** Deferred to V1+ (requires PDF parsing). Current approach: screenshots as future imports.
- ✅ **Account balance endpoint design:** Currently at `PATCH /api/accounts/:id/balance`. 
  - Note: This creates a `_CASH` holding snapshot. With holdings-based balance model, could be clarified as "set cash balance" but works as-is.
  - Schema now supports multi-currency via sub_account, so existing design is compatible.

---

## V0 Burndown Summary

**✅ Backend (Nonso) — SUBSTANTIALLY COMPLETE**

Completed:
- Holdings/Portfolio endpoints fully renamed to /api/holdings/* with dry-run support
- Multiple cash holdings per account (sub_account field, multi-currency via unique constraint)
- Closed holdings support (is_closed flag with index for queries)
- Full account lifecycle: GET, POST, PATCH (balance only)
- Account type enum with 8 types (Checking, Savings, Investment, Credit, Cash, Pension, Property, Mortgage)
- Budget system: standing budgets + monthly overrides with auto-carry via COALESCE queries
- Categories: hierarchical table (parent-child) with full CRUD (POST/GET/PATCH/DELETE/resolve)
- Category to section mappings (Income | Bills | Spending | Irregular | Transfers)
- Transactions: GET/PATCH with category_id FK and exclude_from_summary filtering
- `exclude_from_summary` flag: database field, filtering in all aggregations, PATCH support
- Profile management: GET, POST (no DELETE yet)
- CSV import with bank detection (Monzo, Revolut, Lloyds)
- JSON structured import API (POST /api/import) for external agents
- Bulk import endpoints (POST /api/import/bulk)
- Currency tracking at all levels (transactions, holdings, budgets)
- OpenAPI 3.1 documentation with embedded category taxonomy (GET /api/docs)
- API token generation and validation (bearer token auth)

Done in this PR (2026-05-20):
- **PATCH /api/accounts/:id** (full field set) + **DELETE /api/accounts/:id** (soft-delete, 409 when in use)
- **PATCH /api/profiles/:id** + **DELETE /api/profiles/:id** (default protected, 409 when referenced)
- **PATCH /api/holdings/:account_id/:symbol** expanded to value/currency/sub_account in addition to is_closed; **DELETE /api/holdings/:account_id/:symbol** wired
- **Drop accounts.balance and accounts.balance_date columns** (Option A: table columns dropped, struct fields preserved and runtime-computed from holdings — no frontend change required)

Deferred to V1+:
- **PDF/image document imports** (requires LLM extraction)
- **Transactions dry-run** (only holdings has ?dry_run=true)
- **CSV import dry-run** (would need LLM re-processing or token caching)
- **Generic `Paginated<T>` type** (current endpoints return arrays or single objects)
- **Fingerprint collision disambiguation** (using simple sha256(datetime, amount, account_id))
- **Automatic holding snapshot extraction from imports** (manual entry required)
- **T212 PDF parsing** (no PDF support in V0)

**✅ Frontend (Ope) — BUILD PASSING, CORE COMPLETE**

Completed:
- **Settings page** with 6 sections: Profiles, Accounts, Categories, Data Ingestion, Appearance, Data Source
- **Profile management** (list, create via POST /api/profiles)
- **Account management** (list, create, view balance, type badges)
- **Categories management** (grouped list; backend CRUD endpoints available)
- **Import workflow**: dual modes (wizard with account stepping, single-file mode)
- **File upload** with drag-drop, file dedup by name+size, bank detection feedback
- **Import preview** with stats (total/new/duplicates), error table, dry-run support
- **Budget display** (spending grid with monthly/quarterly/yearly granularity, budget amounts shown)
- **Portfolio view** (holdings summary, account balances, asset allocation)
- **Transactions view** (list, category editing, search/filter)
- **Reports view** (spending by category, cash flow analysis)
- **Navbar**: Import CTA (popover), Settings icon, theme toggle
- **DraggableList** component (reusable, used for account ordering in ingestion preferences)
- **Ingestion preferences** (account ordering, hide/show) persisted to localStorage
- **Live/Mock toggle** (VITE_MOCK_ONLY env var support)
- **Theme toggle** (Light/Dark/System) persisted to localStorage
- **Docker build** (multi-stage: Node frontend, Rust backend, debian-slim runtime)
- **GitHub Actions CI** (lint+build frontend, test+clippy backend)
- **GitHub Actions publish** (auto-version tagging to GHCR on push to master)
- **TypeScript bindings** auto-generated from Rust via ts-rs
- **All TypeScript errors fixed** (mock data updated for sub_account, is_closed, category_id, exclude_from_summary)
- **Frontend builds successfully** without errors

Pending (deferred for later phases):
- **Budget editing UI** (API endpoints ready: setStandingBudget, setBudgetOverride; UI not wired)
- **Skeleton loading states** (components exist, not integrated into all pages)
- **Sticky sidebar nav**
- **Edit/delete buttons** for accounts/profiles (icons present, disabled with "Coming soon" tooltips)
- **E2E Playwright tests** (infrastructure ready, tests not written)
- **Empty categories toggle** (in spending grid)
- **Transaction exclude_from_summary toggle** (disabled with "Coming soon" tooltip, backend ready)
- **Average spend calculation** in budget view
- **Budget hover tooltip** showing spending trend

**Impact:** MVP is ready for early testing. All critical backend features implemented and wired to frontend. Frontend builds without errors and can switch between mock/live API modes. Core workflows fully functional:
- Import CSV files from banks (Monzo, Revolut, Lloyds supported)
- View transactions and categorize them
- Monitor spending via budget grid
- View investment portfolio and cash holdings
- Manage accounts and profiles
Polish items (loading states, edit dialogs, tests) scheduled for next phase.

---

## Still open after 2026-06-14 audit

Everything else in this doc is done in the current code. These are the only items re-confirmed outstanding. None block V0; they are cleanup / polish / deferred-by-design. **Action plan: [docs/plans/27_v0_burndown_cleanup.md](../27_v0_burndown_cleanup.md).**

**Backend cleanup (not tracked elsewhere — needs an active home):**
- **Drop legacy `category` string field** from `Transaction`, `SectionMapping`, `StandingBudget`, `ImportTransaction`, the budget/transactions request bodies, and `unified.rs`. Still marked "kept for backward compat".
- **Holding write model tagged union** (scalar `value` vs `quantity`+`price_per_unit`). Import payload is still flat `Vec<Holding>` with all three fields settable and no consistency validation.
- **Generic `Paginated<T>` struct** (backend) + dropping hand-written `PaginatedResponse<T>` (frontend/src/types/api.ts:55).

**Deferred by design (still intentionally not done):**
- Fingerprint collision disambiguation (`duplicate_index`) — current `sha256(datetime, amount, account_id)` is intentional.
- Image / screenshot uploads — already tracked in docs/plans/20_post_v0_plans.md.
- Cross-file LLM context for multi-file imports — already tracked in docs/plans/20_post_v0_plans.md (multi-file upload itself is done).

**Frontend polish (not tracked elsewhere):**
- Budget hover tooltip (spending trend).
- "Show empty categories" toggle in the budget grid.
- Frontend unit/integration tests for CSV import (only the Playwright smoke script exists).

**Needs verification, not a code gap:**
- "System" theme mode on mobile — code path looks correct (matchMedia + change listener); needs a real-device / emulator re-test to confirm the original bug is gone.


---

# V0 Burndown Cleanup

Action plan for the items left open after the 2026-06-14 re-audit of
`docs/plans/archive/19_v0_burndown.md`. Everything else in that archived doc is
done in the current code; this captures the genuine outstanding work and how to
close it.

## Scope

**In scope (untracked until now):**

1. Drop the legacy `category` string field
2. Holding write-model tagged union (scalar `value` vs `quantity`+`price_per_unit`)
3. Generic `Paginated<T>` (backend) + drop hand-written `PaginatedResponse<T>` (frontend)
4. Budget hover tooltip (spending trend)
5. "Show empty categories" toggle in the budget grid
6. ~~Frontend tests for CSV import~~ — dropped (2026-06-14): CSV import is already covered by backend integration tests; see the resolved note below
7. Verify "System" theme mode on mobile

**Explicitly out of scope (already tracked in `docs/plans/20_post_v0_plans.md`, do not duplicate here):**

- Image / screenshot uploads
- Cross-file LLM context for multi-file imports (multi-file upload itself is done)
- Fingerprint collision disambiguation (`duplicate_index`) — intentional deferral; current `sha256(datetime, amount, account_id)` stays

## Suggested order

Do the cheap, low-risk, file-disjoint items first to bank progress, then the
one large migration last:

1. Generic `Paginated<T>` (item 3) — mechanical, well-specified
2. Holding write-model union (item 2) — contained to holdings write path
3. Budget tooltip + empty-categories toggle (items 4, 5) — frontend only, same file
4. Verify system theme on mobile (item 7) — verification, not code
5. Drop legacy `category` (item 1) — largest, riskiest; do alone, do last

Items 1, 2, and 4/5 are file-disjoint and could run in parallel if delegated.

---

## 1. Drop the legacy `category` string field

**Reality check:** the archived burndown framed this as "remove the field; ts-rs
regenerates bindings; no frontend changes needed." That is wrong. `category` is
not a dead field — it is an active query column:

- `storage/db.rs` reads/filters/searches/sorts on `t.category`,
  `section_mappings.category`, and `standing_budgets.category` in ~20 places
  (display COALESCE, category filter clauses, search `LIKE`, ordering, the
  upsert at the `standing_budgets` ON CONFLICT, and a hardcoded
  `WHERE category = 'Finance: Investment Transfer'` at db.rs:3325).
- Schema columns exist on `transactions`, `section_mappings`, `standing_budgets`;
  migration 7 already made `section_mappings.category` nullable on old DBs.
- Frontend `types/models.ts:72` (legacy hand-written) and all of
  `data/mock_transactions.ts` reference `category`. Generated binding
  `bindings/UnifiedStatementRow.ts` also exposes it.

So this is a data + query migration, not a struct edit.

**Consumers (decided 2026-06-14):** there are no external agents. The frontend
is the only API consumer, and any future integrator reads the API docs, so
updating `docs/api.html` + the OpenAPI spec is the contract. That means a clean
break: drop `category` from the API input outright and update the frontend in
the same PR. No deprecation window on the input is needed.

**Second internal consumer — the CLI.** `fynance budget set --category "<name>"`
(commands/budget.rs:11) and `budget status` (`b.category`) use category *names*,
not IDs, and humans will keep typing names. The CLI should keep accepting a name
and resolve it server-side to `category_id` (throwing a clear error if the name
does not resolve, per the agreed converter behaviour); it must be migrated
alongside the FE, and the old `Budget` model / `set_budget` (model.rs:459,
db.rs:2291) updated.

**Existing-data risk: checked and clear (2026-06-14).** Queried the real DB at
`X:\projects\fynance\data\fynance.db`: of 2592 transactions, **0** have a legacy
`category` string without a `category_id` (1347 carry both; the remainder are
uncategorized). section_mappings (19/19) and standing_budgets (10/10) are fully
on `category_id`. So no row depends on the legacy string — the backfill below is
a safety net, not a data migration, and dropping the columns loses nothing real.

**The hardcoded investment-transfer query is a latent bug this migration fixes.**
`compute_investment_metrics` (db.rs:3301) computes `new_cash_invested` — money
moved *into* investments during the period — by summing transactions with
`category = 'Finance: Investment Transfer'`, so the portfolio view can split
`total_growth` into `market_growth` (real gains) vs contributions. On the real DB
that string matches **0 rows**: the taxonomy category is named `Investment
Transfer` (id `3996e2d7-...`) and the legacy strings don't use that exact
`Finance: …` full-path form, so `new_cash_invested` is effectively always 0
(market growth == total growth). Phase B must resolve this category by id (and
its descendants) and filter `category_id IN (...)`, which removes the legacy
dependency *and* fixes the metric.

**Phased approach:**

- **Phase A — backfill.** Add a migration that, for every row with a non-null
  `category` but null `category_id`, resolves the name to `categories.id` and
  sets `category_id`. Report (don't silently drop) any names that fail to
  resolve. Do the same for `section_mappings` and `standing_budgets`. On the
  current real DB this is a no-op (0 rows need it — see the data check above);
  keep it so any other DB stays safe.
- **Phase B — rewrite reads.** Change every db.rs query that displays/filters/
  searches/sorts by `category` to use `category_id` joined to `categories` for
  the display name. Replace the hardcoded investment-transfer string match with
  a `category_id` lookup (resolve the well-known category once).
- **Phase C — stop writing.** Remove `category` from the insert/update paths;
  the PATCH at db.rs:1582 stops writing the `category` column.
- **Phase D — drop the field + columns.** Remove `category: Option<String>` from
  `Transaction`, `SectionMapping`, `StandingBudget`, `ImportTransaction`, the
  budget request bodies (budget.rs:100/150), the transactions request body
  (transactions.rs:191), and `importers/unified.rs:43`. Add a migration dropping
  the three columns. Regenerate bindings.
- **Phase E — frontend follow-through.** Drop `category` from `types/models.ts`,
  scrub `data/mock_transactions.ts`, and fix any component reading
  `tx.category` (search before assuming none). Update `docs/api.html` and the
  OpenAPI examples so they show `category_id`, not `category`.

**Verification:** `cargo test` + `cargo clippy`; a manual import + transactions
list + budget + spending-grid round trip on a copy of a real DB to confirm
display names still resolve; `tsc --noEmit` + `npm run build`; smoke script.

**Risk:** high. Old databases with unresolvable category names; the
investment-transfer hardcode; mock-data drift. Do this PR alone.

---

## 2. Holding write-model tagged union

**Goal:** the write payload should be either `{ value }` (scalar: cash,
property, loan) or `{ quantity, price_per_unit }` (computed: stock, ETF,
crypto), with the backend deriving `value = quantity * price_per_unit` in the
computed case and rejecting payloads that set fields from both arms. The
response `Holding` stays flat (`value` always set, `quantity`/`price_per_unit`
optional) so reads never pattern-match.

**Current state:** `HoldingsImportPayload { account_id, holdings: Vec<Holding> }`
(model.rs:600) and the POST upsert both take the flat `Holding`, which carries
`quantity: Decimal` (non-optional), `price_per_unit: Option`, and
`value: Decimal` simultaneously with no consistency check.

**Approach:**

- Add an input type in model.rs, e.g. `HoldingWrite` with the common fields
  (account_id, symbol, name, holding_type, currency, as_of, sub_account,
  is_closed, source_document_ids) plus a `HoldingAmount` enum:
  `Scalar { value }` | `Computed { quantity, price_per_unit }`. Prefer an
  internally-tagged or explicitly-validated representation over `#[serde(untagged)]`
  so error messages are clear and "both arms supplied" is rejectable.
- Use `HoldingWrite` for `POST /api/holdings/:account_id` and
  `POST /api/holdings/import` payloads. Derive `value` in the computed arm.
- Keep the internal/parse pipeline producing the flat `Holding` (it already
  computes value); only the external write API gains the union.
- Validation: reject mixed arms with a 400 `invalid_holding`.

**Files:** `model.rs` (new types), `server/routes/holdings.rs` (import_holdings,
the POST upsert, the dry-run preview path), `storage/db.rs` upsert if the
signature changes, `docs/api.html` + OpenAPI examples, bindings regenerate.
Check whether the frontend import-commit flow constructs holding payloads
(`api/real_service.ts`, portfolio/import pages) and update the shape there.

**Verification:** `cargo test` (add cases: scalar-only ok, computed derives
value, both-arms rejected, neither rejected); clippy; `tsc`/build if frontend
payload shape changed; holdings import smoke (dry-run + commit).

**Risk:** medium. API shape change for external callers; coordinate with agents.

---

## 3. Generic `Paginated<T>` + drop `PaginatedResponse<T>`

**Goal:** a single generated pagination envelope shared by Rust and TS, per the
design already written in `docs/plans/archive/13_frontend_backend_handover_unimplemented.md`
section 6.1.

**Current state:** `transactions.rs` defines an inline response struct
(`total`, `page`, `limit` + data) at lines 49-51 / 101-114. The frontend
hand-writes `PaginatedResponse<T>` in `types/api.ts:55`, consumed by
`api/service.ts`, `api/real_service.ts`, `api/mock_service.ts`, and
`hooks/data/use_transactions.ts`.

**Approach:**

- Add `pub struct Paginated<T: TS + 'static> { data: Vec<T>, total: u64, page: u32, limit: u32 }`
  to model.rs with the `#[ts(export)]` derive (ts-rs supports generics →
  `bindings/Paginated.ts`).
- Return `Paginated<Transaction>` from `list_transactions`; delete the inline
  struct.
- Frontend: delete `PaginatedResponse<T>` from `types/api.ts`, re-export
  `Paginated` from `@/bindings/Paginated`, and update the five import sites.

**Verification:** `cargo test` + clippy; regenerate bindings; `tsc --noEmit` +
`npm run build`; transactions table paging smoke.

**Risk:** low, mechanical. Confirm no other endpoint silently relied on the old
inline shape.

---

## 4. Budget hover tooltip (spending trend)

**Goal:** hovering a budget/spending cell shows the recent per-period trend for
that category.

**Approach:** in `frontend/src/pages/budget/budget_spreadsheet.tsx`, wrap the
relevant cell in the shared `Tooltip` (`components/ui/tooltip`) with a compact
table or mini-sparkline of recent months. The spending-grid response already
carries per-period values per row; reuse them rather than fetching. Mirror the
existing CostTag tooltip pattern referenced by the smoke script
(`[data-slot=tooltip-content] table`).

**Verification:** `tsc`/build; extend `scripts/smoke_preview.mjs` to hover the
budget cell and assert the tooltip table renders; screenshots to `.playwright-mcp/`.

**Risk:** low.

---

## 5. "Show empty categories" toggle (budget grid)

**Goal:** a toggle that hides categories with no budget and no spend in the
visible range; default hidden.

**Approach:** add a `Switch` in the budget grid header and filter rows where
budget and actual are both zero across the visible periods, in
`budget_spreadsheet.tsx`. Persist the choice the same way other view prefs are
persisted (localStorage), matching the ingestion-preferences pattern.

**Verification:** `tsc`/build; add a smoke step toggling it; screenshots.

**Risk:** low.

---

## 6. (Resolved) Import tests already exist — no new CSV tests needed

Decided 2026-06-14: do not add frontend CSV tests. CSV import is already covered
on the backend:

- `backend/tests/import_csv.rs` — full importer → storage → sqlite integration
  test, driven by `MockStatementParser` seeded from
  `tests/fixtures/{monzo,revolut,lloyds}.expected.json` (runs without an API key).
- `backend/tests/parse_multipart.rs` — the `/api/parse` multipart flow.
- In-module `#[test]`s in `importers/` (format_detection, llm_parser,
  holdings_parser, etc.).

**Frontend:** the import path *is* tested. `frontend/scripts/smoke_preview.mjs`
is a Playwright script that drives import → account select → file upload → parse
(it asserts the progress bar advances) → preview → submit → confirm → "import
complete", in mock mode, across desktop + mobile and light + dark. It's a smoke
script rather than a unit-test runner, but it covers "the import path works."
Nothing to add for V0 cleanup.

---

## 7. Verify "System" theme mode on mobile

**Not a code gap — a verification task.** `frontend/src/hooks/use_theme.ts`
already resolves `system` via `matchMedia("(prefers-color-scheme: dark)")` and
subscribes to changes, so the path looks correct. The original report ("System
mode does not auto-detect on mobile") cannot be confirmed or refuted statically.

**Approach:** the smoke script already runs the mobile viewport (390×844) but
sets an explicit `light`/`dark` theme, so it doesn't exercise `system`
auto-detect. Add a variant that stores theme = `system`, emulates
`prefers-color-scheme` dark then light, and asserts the `<html>` class follows.
If it reproduces, the usual culprit is the `matchMedia` change subscription
(older iOS Safari needs `addEventListener` / `addListener` fallbacks) — check
`use_theme.ts` uses a listener API the target browsers support. Otherwise mark
the burndown note resolved.

**Verification:** Playwright mobile run with color-scheme emulation; screenshots.

**Risk:** low.

---

## Open decisions for the user

1. **Legacy `category` (item 1):** consumers resolved (FE + docs + CLI), and the
   data check is clear (no row depends on the legacy string), so dropping the
   three columns now is safe and loses nothing. Recommend the column drop in the
   same PR; the migration also fixes the dead investment-transfer metric. No
   real decision left here unless you'd rather stage the column drop separately.
2. **Holding union (item 2):** confirm the shape — internally-tagged enum with
   explicit "both arms supplied" rejection, vs `#[serde(untagged)]`.
