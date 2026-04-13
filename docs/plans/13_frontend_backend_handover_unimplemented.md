# Frontend-Backend Handover: Unimplemented Asks

**Date:** 2026-04-12  
**Source:** `/docs/frontend-backend-handover.md` (full requirements analysis)  
**Status:** Items below have been identified as mentioned in the handover but not yet fully implemented in the codebase.

---

## Summary

Of the 53 backend asks identified in the frontend-backend-handover document, the architectural consolidation of `portfolio_snapshots` into `holdings` has been completed (see Completed section at the bottom). The following areas remain unimplemented and require attention:

1. **Multiple Cash Holdings Per Account** — Pots / sub-balances / multi-currency need schema or convention fix; currently blocked by `UNIQUE(account_id, symbol, as_of)` + the fixed `_CASH` sentinel
2. **CSV Import Enhancements** — Extend the importer to extract balance and holdings data
3. **Currency and Exchange Rate Handling** — Add exchange rate capture and currency conventions

---

## Section 1: Multiple Cash Holdings Per Account (Pots / Sub-balances)

**Current State:**
Migration 004 consolidated `portfolio_snapshots` into `holdings` by inserting one row per account per date with `symbol = '_CASH'`. Combined with the existing `UNIQUE(account_id, symbol, as_of)` constraint on `holdings`, this means **only one cash holding per account per date is possible**. A second `_CASH` row for the same account on the same `as_of` is rejected by the unique index.

**Why This Is a Gap:**
The original handover (`docs/frontend-backend-handover.md` Section 7) explicitly called out pots, vaults, and multi-currency balances as cash holdings that should live inside a single parent account:

```
Account: "Monzo Current" (balance: £2,500)
  |-- Holding: "Main balance"   (cash, £590)
  |-- Holding: "Bills pot"      (cash, £800)
  |-- Holding: "Holiday pot"    (cash, £600)
  |-- Holding: "Emergency pot"  (cash, £510)
```

Under the current schema all four rows collide on `(account_id='monzo-current', symbol='_CASH', as_of='2026-03-15')`. The same issue affects a Revolut account that holds GBP, EUR, and USD balances on the same day.

**What's Needed:**
Pick one of the following. Option A is the cheapest and fully unblocks pots:

- **Option A — Use meaningful symbols instead of a fixed `_CASH` sentinel.** Cash holdings can use symbols like `POT_BILLS`, `POT_HOLIDAY`, or the pot's short name. Plain accounts with no pots keep a single `_CASH` row for the whole balance. No schema change, just a convention shift in the importer and in migration 004's intent.
- **Option B — Widen the unique constraint to `UNIQUE(account_id, symbol, name, as_of)`.** Allows multiple `_CASH` rows differentiated by `name` (e.g. "Main balance", "Bills pot"). Schema migration required.
- **Option C — Add a dedicated `slot` or `label` column** and include it in the unique key.

**Acceptance Criteria:**
- [ ] A single account can carry multiple cash holdings on the same `as_of` date
- [ ] Ingestion of a Monzo-style account with pots produces one holding row per pot, not a single merged balance
- [ ] Multi-currency accounts (e.g. Revolut GBP/EUR/USD) can represent each currency as its own cash holding on the same date
- [ ] Existing `_CASH` rows migrated from `portfolio_snapshots` continue to work

**Priority:** Medium (blocks any pot-aware or multi-currency feature; not blocking the current transactions-only MVP flow)

---

## Section 2: CSV Import Enhancements

### 2.1 CSV Import: Extract Balance Data

**Current State:**  
The CSV importer only extracts transactions. Although bank CSV exports often include a running or closing balance (`balance_after` field), this data is parsed but **not stored as a portfolio snapshot**.

**What's Needed:**  
When a CSV is imported and a `balance_after` or closing balance is available, the importer should:
1. Extract the closing balance (or most recent balance in the file)
2. Create a `portfolio_snapshot` row with:
   - `snapshot_date`: the date of the balance
   - `account_id`: the account being imported
   - `balance`: the closing balance amount
   - `currency`: the currency of that balance

**Implementation Notes:**
- The LLM parser already extracts `balance_after` per transaction (see `llm_parser.rs` line 292-295)
- For the closing balance, take the last transaction's `balance_after` in the file
- The `portfolio_snapshots` table already has the schema in place (db/sql/schema.sql lines 64-71)
- This should happen within the unified importer flow in `importers/unified.rs`

**Acceptance Criteria:**
- [ ] Running a CSV import creates a portfolio_snapshot row with the final balance as of the CSV's last transaction date
- [ ] If no balance_after is available in the CSV, no snapshot is created (no error; graceful degradation)
- [ ] Tests verify snapshots are created for accounts and dates that don't already have one

**Priority:** Medium (medium MVP impact — balances can currently only be set manually via `account set-balance`)

---

### 2.2 CSV Import: Extract Holdings Data

**Current State:**  
Investment account CSVs often include position data (e.g., "100 shares of AAPL @ $150"). The importer does **not** currently extract or create holdings records from this data.

**What's Needed:**  
The LLM parser and unified importer should recognize holdings/position data in investment account exports and:
1. Extract symbol, name, quantity, price_per_unit, value, and holding_type
2. Create or upsert `holdings` rows with:
   - `account_id`: the account
   - `symbol`: the stock/fund/crypto symbol
   - `name`: the human-readable name
   - `holding_type`: 'stock', 'fund', 'crypto', 'bond', etc.
   - `quantity`: shares or units held
   - `price_per_unit`: cost per unit (if available)
   - `value`: total value of the holding
   - `currency`: the currency of the value
   - `as_of`: the date of the snapshot

**Implementation Notes:**
- The `holdings` table already exists (db/sql/schema.sql lines 89-107)
- The LLM `statement_parser.txt` prompt must be updated to include instructions for extracting holdings rows in addition to transactions
- The tool schema in `llm_parser.rs` should include optional holdings extraction (new variant or table schema)
- Holding detection should be intelligent: recognize headers like "Position", "Holdings", "Assets", "Security", "Symbol", etc.
- This is prompt engineering work, not a new importer

**Acceptance Criteria:**
- [ ] The LLM parser can extract holdings data from investment account CSVs
- [ ] Holdings are created with correct symbol, quantity, value, and as_of date
- [ ] Repeated imports of the same holdings CSV update existing holdings (UPSERT semantics)
- [ ] Tests verify holdings extraction for sample investment CSV formats (Trading 212, Vanguard, etc.)

**Priority:** Medium (post-MVP phase for portfolio detail; transaction-only import still works)

---

### 2.3 LLM Parser: Extend `statement_parser.txt` Prompt

**Current State:**  
The system prompt at `backend/src/importers/statement_parser.txt` is designed for transaction extraction only.

**What's Needed:**
1. **Add holdings extraction rules** to the prompt (similar to transaction extraction rules)
2. **Teach the parser** to recognize common investment account CSV formats (closing balance lines, position tables)
3. **Define output format** for holdings (new JSON key in the tool call output, or separate tool call)
4. **Handle edge cases:**
   - CSV with only a closing balance line (no transaction detail)
   - CSV with position data but no transaction history
   - CSV with both transactions and positions

**Example Enhancement:**
```
Holdings Extraction (if present):
- Look for sections with headers: "Holdings", "Portfolio", "Positions", "Securities", "Assets"
- Each row should extract: symbol, name, quantity, price_per_unit, value, currency, as_of_date
- Ignore transaction-like rows; focus on position snapshots
- If a "closing balance" or "total value" line exists, use its date as the as_of date for all holdings
```

**Priority:** Medium (paired with 2.2)

---

## Section 3: Currency and Exchange Rate Handling

### 3.1 Exchange Rate Capture Table

**Current State:**  
The database schema has **no exchange_rates table**. While individual transactions store their currency, there is no way to track what exchange rate was used (if any) or for historical currency conversions.

**What's Needed:**
Create an `exchange_rates` table to store FX rates at ingestion time:

```sql
CREATE TABLE IF NOT EXISTS exchange_rates (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    base_currency   TEXT NOT NULL,              -- e.g., 'GBP'
    quote_currency  TEXT NOT NULL,              -- e.g., 'USD'
    rate            TEXT NOT NULL,              -- Decimal, never float
    as_of_date      TEXT NOT NULL,              -- the date the rate was valid
    source          TEXT NOT NULL DEFAULT 'manual',  -- 'manual', 'api', etc.
    created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    UNIQUE(base_currency, quote_currency, as_of_date, source)
);

CREATE INDEX IF NOT EXISTS idx_ex_currencies ON exchange_rates(base_currency, quote_currency);
CREATE INDEX IF NOT EXISTS idx_ex_date ON exchange_rates(as_of_date);
```

**Implementation Notes:**
- Rates should be stored as Decimal (TEXT) to avoid floating-point error, matching the rest of the codebase
- One row per (base, quote, date) pair to support historical rate changes
- Populate at import time if a rate is known (e.g., from bank metadata) or leave empty (rates can be looked up later)
- This table enables future multi-currency reporting and historical accuracy

**Acceptance Criteria:**
- [ ] Table is created in schema.sql
- [ ] No schema enforcement requiring rates (optional for MVP); rates can be added manually later
- [ ] Table is indexed for fast lookups by currency pair and date

**Priority:** High (establish convention early; avoids retroactive data loss)

---

### 3.2 Enforce: All Values Stored in Source Currency

**Current State:**  
The codebase stores each transaction with its declared currency (good), but there is **no validation** that currencies are never converted at ingestion time. The handover emphasizes: "Never convert at ingestion time, because exchange rates change and you would lose the original value."

**What's Needed:**
1. **Add validation logic** in the importer to reject any conversion directives
2. **Document in code and commit messages** that all monetary values are stored in their source currency
3. **Ensure rules and categorization** do not alter amounts (only category/notes)
4. **Update API docs** (OpenAPI spec) to clarify that amounts are always in the source currency of the transaction

**Example Validation:**
- If a CSV import extracts transactions in multiple currencies for the same account, accept all (don't convert)
- If a user tries to import pre-converted amounts, log a warning and reject the import with a clear error message

**Implementation Notes:**
- This is a convention/validation issue, not a schema change
- Update CLAUDE.md and code comments to reinforce this principle
- Multi-currency net worth calculations happen at query time via `exchange_rates` table (not at import)

**Acceptance Criteria:**
- [ ] Code comments and docs explicitly state: "All amounts stored in source currency, never converted at ingestion"
- [ ] Importer tests verify that multi-currency transactions are accepted as-is
- [ ] API docs clarify currency handling in responses

**Priority:** High (set expectations early)

---

### 3.3 Display-Time Currency Conversion (Post-MVP)

**Current State:**  
Out of scope for MVP. The backend stores all values in source currency; the frontend eventually needs to support toggling between source and user-preferred currency for display.

**What's Needed (Post-MVP):**
- Backend endpoint or helper to look up exchange rates for a given date and currency pair
- Frontend logic to convert displayed amounts to user's preferred currency
- Ability to set user preferred currency in profile settings

**Acceptance Criteria (Post-MVP):**
- [ ] User can select preferred display currency in settings
- [ ] All monetary values in UI show converted amount (with source amount as tooltip)
- [ ] Conversion uses the exchange_rates table for historical accuracy

**Priority:** Low (post-MVP, deferred)

---

## Section 3: Architectural Consolidation Decision

### 3.1 Consolidate snapshots into holdings

**Current State:**  
The database has two separate time-series tables:
- `portfolio_snapshots`: account-level balance at a point in time
- `holdings`: security-level details (shares, values) at a point in time

Both support carry-forward semantics (most recent value as of a date), and there is some overlap in what they represent.

**Decision Needed:**  
The handover proposes consolidating these into a single `holdings` table as the source of truth for balances:
- Account balance = SUM(all holdings for that account as of a date)
- Each account has at least one "cash" or "account balance" holding
- Eliminates redundancy and parallel time-series logic

**Current Blocker:**  
- This is a **major architectural change** requiring sign-off from both frontend and backend leads
- If adopted, `portfolio_snapshots` should be **dropped** (or deprecated)
- If not adopted, `portfolio_snapshots` should be **renamed** to `account_snapshots` for clarity

**Implementation Notes (if consolidation is adopted):**
- [ ] Add `"cash"` variant to the `HoldingType` enum (one-line Rust change)
- [ ] Update schema migration to drop `portfolio_snapshots` table
- [ ] Update all portfolio queries to SUM holdings instead of reading snapshots
- [ ] Update import logic to create cash holdings instead of snapshots
- [ ] Delete portfolio_snapshots.rs and related code

**Implementation Notes (if consolidation is rejected):**
- [ ] Rename `portfolio_snapshots` table to `account_snapshots` for clarity
- [ ] Update all references in code (routes, queries, etc.)
- [ ] Leave both tables in place; no further consolidation planned

**Acceptance Criteria (must choose one):**
- [ ] **Option A (Consolidate):** HoldingType includes 'cash', portfolio_snapshots dropped, all balance queries use holdings
- [ ] **Option B (Rename Only):** portfolio_snapshots renamed to account_snapshots, semantics unchanged

**Priority:** High (architectural decision, must be made before phase 2 of new features)

**Status:** Requires decision from Nonso; awaiting sign-off.

---

## Section 3.2: Multiple Cash Holdings Per Account (Pots / Sub-balances)

**Current State:**
Migration 004 consolidated `portfolio_snapshots` into `holdings` by inserting one row per account per date with `symbol = '_CASH'`. Combined with the existing `UNIQUE(account_id, symbol, as_of)` constraint on `holdings`, this means **only one cash holding per account per date is possible**. A second `_CASH` row for the same account on the same `as_of` is rejected by the unique index.

**Why This Is a Gap:**
The original handover (`docs/frontend-backend-handover.md` Section 7) explicitly called out pots, vaults, and multi-currency balances as cash holdings that should live inside a single parent account:

```
Account: "Monzo Current" (balance: £2,500)
  |-- Holding: "Main balance"   (cash, £590)
  |-- Holding: "Bills pot"      (cash, £800)
  |-- Holding: "Holiday pot"    (cash, £600)
  |-- Holding: "Emergency pot"  (cash, £510)
```

Under the current schema all four rows collide on `(account_id='monzo-current', symbol='_CASH', as_of='2026-03-15')`. The same issue affects a Revolut account that holds GBP, EUR, and USD balances on the same day.

**What's Needed:**
Pick one of the following. Option A is the cheapest and fully unblocks pots:

- **Option A — Use meaningful symbols instead of a fixed `_CASH` sentinel.** Cash holdings can use symbols like `POT_BILLS`, `POT_HOLIDAY`, or the pot's short name. Plain accounts with no pots keep a single `_CASH` row for the whole balance. No schema change, just a convention shift in the importer and in migration 004's intent.
- **Option B — Widen the unique constraint to `UNIQUE(account_id, symbol, name, as_of)`.** Allows multiple `_CASH` rows differentiated by `name` (e.g. "Main balance", "Bills pot"). Schema migration required.
- **Option C — Add a dedicated `slot` or `label` column** and include it in the unique key.

**Acceptance Criteria:**
- [ ] A single account can carry multiple cash holdings on the same `as_of` date
- [ ] Ingestion of a Monzo-style account with pots produces one holding row per pot, not a single merged balance
- [ ] Multi-currency accounts (e.g. Revolut GBP/EUR/USD) can represent each currency as its own cash holding on the same date
- [ ] Existing `_CASH` rows migrated from `portfolio_snapshots` continue to work

**Priority:** Medium (blocks any pot-aware or multi-currency feature; not blocking the current transactions-only MVP flow)

---

## Section 4: Balance vs Holdings Mismatch Handling

### 4.1 Soft Warning for Unaccounted Balance

**Current State:**  
The code can display account holdings (e.g., 100 AAPL + 50 USD cash), but if the total value of holdings doesn't match the account balance, there's no warning or validation.

**Scenario:**  
- Account balance: £10,000
- Holdings value: £7,500
- Unaccounted: £2,500 (where did it go?)

**What's Needed:**  
If consolidation is NOT adopted (portfolio_snapshots remains), add a soft warning to the portfolio response:
```rust
pub struct PortfolioResponse {
    // ... existing fields ...
    
    /// Optional warning if holdings sum doesn't match account balance
    pub unaccounted_balance: Option<UnaccountedBalance>,
}

pub struct UnaccountedBalance {
    pub account_id: String,
    pub account_balance: Decimal,
    pub holdings_total: Decimal,
    pub difference: Decimal,
    pub message: String,  // e.g., "Holdings total £7.5k but account balance is £10k. Check import completeness."
}
```

**Implementation Notes:**
- This is **optional for MVP** if consolidation is adopted (holdings ARE the balance, so no mismatch possible)
- Query: fetch account balance and sum of holdings; if difference > 0.01, populate warning
- Frontend can display as a yellow warning icon or "Balance mismatch" indicator
- User can dismiss or investigate via detailed holdings list

**Acceptance Criteria:**
- [ ] If holdings_total < account_balance, warning is shown with difference amount
- [ ] Warning appears in portfolio response when summary mode is used
- [ ] Tests verify warning is correctly calculated

**Priority:** Low (nice-to-have; relevant only if consolidation is not adopted)

---

## Section 5: Implementation Roadmap

### Phase 3a: CSV Import Enhancements (Medium Priority)

**Depends on:** Nothing (can be done in parallel)

**Changes Required:**
1. Extend `llm_parser.rs` tool schema to optionally extract balance_after as closing balance
2. Update `statement_parser.txt` to instruct LLM to output closing balance
3. Modify unified importer to create portfolio_snapshots after transaction import
4. Update holdings parser (same files) to recognize position data
5. Add tests for balance and holdings extraction

**Affected Files:**
- `backend/src/importers/llm_parser.rs`
- `backend/src/importers/statement_parser.txt`
- `backend/src/importers/unified.rs`
- Tests

**Estimated Effort:** Medium (prompt engineering + query logic, no database changes)

---

### Phase 3b: Currency and Exchange Rate Handling (High Priority)

**Depends on:** Nothing (can be done in parallel)

**Changes Required:**
1. Add `exchange_rates` table to schema.sql and migrations
2. Add validation in importer to reject pre-converted amounts
3. Document currency handling conventions in code and CLAUDE.md
4. Update API docs to clarify currency in responses

**Affected Files:**
- `db/sql/schema.sql`
- `db/sql/migrations/*.sql` (new migration)
- `backend/src/importers/csv_importer.rs` (validation)
- `CLAUDE.md` (documentation)
- OpenAPI spec (if maintained manually)

**Estimated Effort:** Medium (schema + validation logic, no breaking changes to existing code)

---

### Phase 4: Architectural Consolidation (High Priority, Decision Required)

**Depends on:** Decision from Nonso and Ope on consolidation proposal

**Changes Required (if consolidated):**
1. Add `"cash"` to `HoldingType` enum (1 line change)
2. Drop `portfolio_snapshots` table in migration
3. Update all portfolio queries to `SUM(holdings) GROUP BY account_id`
4. Update importer to create cash holdings instead of snapshots
5. Delete snapshot-specific code (routes, db methods, etc.)

**Changes Required (if NOT consolidated):**
1. Rename `portfolio_snapshots` to `account_snapshots` (db + code)
2. Update all references (straightforward search-replace)

**Affected Files (if consolidated):**
- `backend/src/model.rs` (HoldingType enum)
- `backend/src/db.rs` (portfolio queries, drop snapshot methods)
- `backend/src/routes/portfolio.rs` (routing)
- `backend/src/importers/unified.rs` (create holdings instead of snapshots)
- `db/sql/migrations/` (drop table)
- All tests referencing portfolio_snapshots

**Estimated Effort:** Medium (extensive but straightforward refactoring)

**Blocking:** This decision must be made before phase 3 features go into production, as the schema change is large.

---

## Section 6: Type-Sharing Follow-ups (Frontend → Backend Ask)

These are small backend changes that would let the frontend drop the last
hand-written interfaces that duplicate backend shapes. The goal is a single
source of truth: if the backend owns the wire format, the frontend should
import the ts-rs binding, not maintain its own copy.

### 6.1 `Paginated<T>` envelope for `GET /api/transactions`

**Current state:** The backend route at `backend/src/server/routes/transactions.rs`
builds the list response with an inline `serde_json::json!` macro:

```rust
Ok(Json(serde_json::json!({
    "data": data,
    "total": total,
    "page": q.page,
    "limit": q.limit,
})))
```

Because the return type is `serde_json::Value`, there is no Rust struct to
annotate with `#[ts(export)]`, so no binding is generated. The frontend
maintains a hand-written generic:

```typescript
// frontend/src/types/api.ts
export interface PaginatedResponse<T> {
  data: T[]
  total: number
  page: number
  limit: number
}
```

The two happen to match today, but nothing enforces it. If the backend
adds `total_pages` or renames `limit` to `page_size`, the frontend will
silently break.

**Ask:** Introduce a generic `Paginated<T>` struct and use it as the
return type:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../frontend/src/bindings/")]
pub struct Paginated<T: TS + 'static> {
    pub data: Vec<T>,
    pub total: u64,
    pub page: u32,
    pub limit: u32,
}

pub async fn list_transactions(...) -> Result<Json<Paginated<Transaction>>, AppError> {
    ...
    Ok(Json(Paginated { data, total, page: q.page, limit: q.limit }))
}
```

ts-rs supports generics, so this generates a `Paginated.ts` binding. The
frontend then drops `PaginatedResponse<T>` from `types/api.ts` and imports
`Paginated` from `@/bindings/Paginated` instead. Any future paginated
endpoint (accounts, import log, holdings history) gets the same shape for
free.

**Impact:** Low risk (return shape is identical, clients unaffected),
high consistency value. Roughly 20 lines of backend code.

### 6.2 `SetStandingBudgetBody` and `SetBudgetOverrideBody` — done

These were resolved in the same PR as this note. Both structs in
`backend/src/server/routes/budget.rs` now derive `TS` and export bindings,
and the frontend re-exports them via `types/api.ts` instead of carrying a
hand-written `BudgetUpdateRequest` interface. The `ApiService.updateBudget`
method was also split into `setStandingBudget` and `setBudgetOverride`
matching the two backend endpoints. Noting here only as context for the
pattern above.

### 6.3 Remaining frontend-only types (correctly frontend-only)

For the record, the frontend still maintains a few types that do not have
backend equivalents and should stay that way:

| Type | Why frontend-only is correct |
|---|---|
| `TransactionFilters` | Query-string shape for `GET /api/transactions`. The backend deserializes each param individually via `Query<ListTransactionsQuery>`. It is a serialization *input*, not a response. |
| `CategoryTotalFilters` | Same reasoning as above, for `GET /api/transactions/by-category`. |
| `DateRange` | Pure UI state object used by the date picker. Never crosses the wire. |

Forcing these to share a type with the backend query structs would be a
category error — they serve opposite directions of the wire.

---

## Summary Table

| Item | Status | Priority | Effort | Decision Required |
|------|--------|----------|--------|-------------------|
| CSV import: extract balance data | Not implemented | Medium | Small | No |
| CSV import: extract holdings data | Not implemented | Medium | Medium | No |
| LLM parser: extend for holdings | Not implemented | Medium | Medium | No |
| Exchange rates table | Not implemented | High | Small | No |
| Enforce source currency storage | Not implemented | High | Small | No |
| Display-time currency conversion | Out of scope (post-MVP) | Low | Medium | N/A |
| Consolidate snapshots into holdings | Requires decision | High | Medium | **Yes** |
| Soft warning for balance mismatch | Not implemented | Low | Small | No |

---

## Next Steps

1. **Immediately:** Decide on architectural consolidation (Section 3.1) — consolidate or rename?
2. **Phase 3a:** Implement CSV import enhancements (balance + holdings extraction)
3. **Phase 3b:** Implement currency and exchange rate handling
4. **Phase 4:** Execute architectural consolidation decision
5. **Tiny, any time:** Section 6.1 — add `Paginated<T>` generic struct with ts-rs so the frontend can drop `PaginatedResponse<T>`.
6. **Post-MVP:** Display-time currency conversion in frontend

---

## References

- Frontend-Backend Handover: `/docs/frontend-backend-handover.md`
- Current Implementation Plan: `/docs/plans/09_backend_implementation_plan.md`
- Data Model Design: `/docs/design/03_data_model.md`
- Database Schema: `/db/sql/schema.sql`
- Importer Code: `/backend/src/importers/`
- Consolidation Proposal: `/docs/plans/11_frontend_backend_consolidation.md`
