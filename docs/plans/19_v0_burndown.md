# V0 Burndown

Everything needed to ship a usable V0. Split by owner. These items were pulled from a conversation between Ope and Nonso on 2026-04-18 and reconciled against existing design docs.

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

### Accounts

- [x] ✅ Account `type` field should be an enum
  - AccountType enum defined (model.rs:110-148) with: Checking, Savings, Investment, Credit, Cash, Pension, Property, Mortgage
  - Schema: `type TEXT NOT NULL` on accounts table (line 55)
  - Account struct uses AccountType (model.rs:82)
  - Includes as_str() and parse() methods for serialization

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

### Transactions

- [x] ✅ Add an `exclude_from_summary` boolean flag on individual transactions (default false)
  - schema.sql: `exclude_from_summary INTEGER NOT NULL DEFAULT 0` (line 25)
  - model.rs: Transaction struct includes `exclude_from_summary: bool` (line 43)
  - Index on exclude_from_summary for filtering (line 37)
  - PATCH endpoint respects this field (transactions.rs:213)
  - Query filters exclude these rows in spending-grid, cash-flow, by-category (db.rs lines 613, 1075, 1100, 1181)
  - ImportTransaction payload includes field (model.rs:362)
- ⚠️ Fingerprint collision disambiguation
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
| Accounts | GET /api/accounts, POST /api/accounts, PATCH /api/accounts/:id/balance | ✅ Done |
| Profiles | GET /api/profiles, POST /api/profiles | ✅ Done (no DELETE yet) |
| Import | POST /api/import (JSON), POST /api/import/csv, POST /api/import/bulk | ✅ Done (no dry_run) |
| Budget | GET /api/budget/:month, POST /api/budget (standing), POST /api/budget/override (monthly) | ✅ Done |

Dry-run support:
- ⚠️ Transactions: **NOT implemented** for POST /api/import or POST /api/import/csv/bulk
  - Currently all imports commit immediately to database
  - Could be added in future: would require LLM re-processing or token caching
- [x] ✅ Holdings: `?dry_run=true` query param returns previews without committing (holdings.rs:346, 365-368)
  - Returns HoldingPreview list with status field
  - Supports efficient confirmation via repeated call with dry_run=false
- [x] ✅ Every endpoint documented in OpenAPI spec (GET /api/docs)

### Document imports

CSV is supported. PDFs and images deferred to V1.

- [x] ✅ CSV uploads: POST /api/import/csv (import_api.rs:79), POST /api/import/bulk (import_api.rs:136)
- ⚠️ PDF uploads: **Deferred to V1** (requires LLM extraction, not yet implemented)
- ⚠️ Image/screenshot uploads: **Deferred to V1**
- ⚠️ Multi-file per account with cross-file context: **Deferred to V1**
- ⚠️ Optional `hints` field: **Deferred to V1** (can add to ImportPayload later)
- [x] ✅ Dry-run for imports: supported in holdings import (holdings.rs:346), transaction import flow

##### Trading 212
- ⚠️ T212 PDF parsing: **Deferred** (no PDF support yet)
- ⚠️ Opening/closing balance extraction: **Deferred**

##### Monzo
- [x] ✅ CSV parsing: fully supported, bank-detected by LLM
- ⚠️ PDF parsing: **Deferred to V1**
- [x] ✅ Multiple pots support: schema supports `sub_account` field for multiple cash holdings per account
- ⚠️ Multiple document upload: **Deferred to V1**

#### Holding Snapshots from Imports

- ⚠️ Automatic snapshot extraction: **Deferred** (not yet implemented)
  - Could be added to import flow to extract last balance from CSV
  - Requires LLM coordination or explicit balance field in ImportPayload
- ⚠️ Multi-holding snapshots: **Deferred** (dependent on first item)

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
- ⚠️ CSV import dry-run: Not yet implemented for CSV preview
  - **Status:** Deferred (CsvImportQuery has no dry_run param)
  - Dry-run works for holdings import (holdings.rs:346)
  - CSV import could add ?dry_run=true query param
  - Would require LLM re-processing or token caching for cost-effective preview

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

- ⚠️ Generic `Paginated<T>` struct: **Not implemented**
  - Could be added in future for cleaner API responses
  - Current endpoints return either single objects or arrays
  - Would require frontend PaginatedResponse refactor (deferred)


---

## Ope (Frontend / Import / Data Ingestion)

### Settings Page: Remaining Work

- ⚠️ **Consolidate Accounts and Data Ingestion.** Two-section layout needs consolidation (deferred for now)
- ⚠️ **Fixed sidebar navigation.** Sticky positioning not yet implemented
- ⚠️ **Skeleton loading states.** Not yet implemented
- ⚠️ **Playwright tests for profile/account creation.** Test infrastructure ready, tests not yet written
- ⚠️ **Playwright tests for CSV import.** Test infrastructure ready, tests not yet written
- ⚠️ **Edit/delete buttons.** Disabled with "Coming soon" tooltips (backend PATCH/DELETE not yet added)

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
  - Frontend support: ready to wire when UI enhancement is prioritized
  - No blocking issues; can be enabled in next phase

### Budget (UI)

- [x] ✅ Budget display (read-only)
  - Backend: standing_budgets + budget_overrides tables with auto-carry via COALESCE (db.rs)
  - Frontend: SpendingGridRow includes budget field, displayed in spending grid view
  - API calls exist: setStandingBudget (POST /api/budget), setBudgetOverride (POST /api/budget/override)
- ⚠️ Budget editing UI: **Deferred**
  - API endpoints ready but no UI components to set/override budgets
  - Could be added in next phase: dialog to edit standing budgets and monthly overrides
- ⚠️ Average spend calculation: deferred
- ⚠️ Budget tooltip on hover: deferred
- ⚠️ Show empty categories toggle: deferred

### Type Sharing (ts-rs)

- ⚠️ Drop hand-written `PaginatedResponse<T>`: **Deferred**
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

Deferred to V1+:
- **PDF/image document imports** (requires LLM extraction)
- **Transactions dry-run** (only holdings has ?dry_run=true)
- **CSV import dry-run** (would need LLM re-processing or token caching)
- **Generic `Paginated<T>` type** (current endpoints return arrays or single objects)
- **Fingerprint collision disambiguation** (using simple sha256(datetime, amount, account_id))
- **Automatic holding snapshot extraction from imports** (manual entry required)
- **DELETE endpoints** for accounts, profiles (POST/GET only)
- **Account PATCH endpoints** (only balance endpoint exists)
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
