# 28 - Account/Holding type cleanup + test sign-off

Outstanding work as of 2026-07-04, branch `feat/settings-collapsible-categories`.

Two independent tracks:

- **Part A** is a settled-but-not-started change: prune confusing type-enum variants and add startup migrations.
- **Part B** is a manual test sign-off list for features already built and merged into this branch. Each item points at the file that implements it.

Backend runs on `127.0.0.1:7433`, frontend dev on `:5173`. The real dataset lives at `X:/projects/fynance/data/fynance.db` (the default DB path is empty). To exercise real data, start the backend with `FYNANCE_DB_PATH=X:/projects/fynance/data/fynance.db cargo run -- serve --no-open`. Never read/write the DB directly; go through the REST API or CLI.

---

## Part A - Type enum cleanup (settled, not started)

### Why

Three enum variants are confusing or redundant and should go:

1. `AccountType::Cash` overlaps with `Checking` and adds no signal. Drop it; existing `cash` accounts become `checking`.
2. `HoldingType::Savings` has no data source in practice (savings accounts store their balance as a `cash` holding), yet it is the only input to the Net Savings metric, so that metric reads 0. Drop the holding type and re-derive Net Savings from savings/emergency-fund **accounts** instead.
3. `HoldingType::Loan` and `HoldingType::Credit` are two names for the same thing (a liability line). Holding types are never user-picked (they are assigned by the fuzzy parser at import), so the duplication only causes data-model confusion. Merge both into a single `HoldingType::Liability`. The `AccountType::Credit` (credit card account) is unaffected and stays.

### Settled decisions

- Drop `AccountType::Cash`. Migrate existing `cash` accounts to `checking` on startup.
- Drop `HoldingType::Savings`. Migrate existing `savings` holdings to `cash` on startup.
- Merge `HoldingType::Loan` + `HoldingType::Credit` into new `HoldingType::Liability`. Migrate existing `loan` and `credit` holdings to `liability` on startup.
- **Net Savings** (`compute_savings_growth`) is redefined: sum the carry-forward balances of accounts whose type is `savings` or `emergency_fund` at `end` minus the same at `start`. No dependency on any holding type.
- Migrations run inside `migrate_schema()` (called from `Db::init`, `backend/src/storage/db.rs:171`, before the server serves), as idempotent `UPDATE`s (safe to run every startup: after the first pass no rows match).

### Real-data impact (verify after)

Current counts on `X:/projects/fynance/data/fynance.db` (via API):
- Accounts: `cash` = 0 (so the account migration is a no-op here), plus `checking` 7, `savings` 3, `investment` 6, `investment_isa` 2, `credit` 1, `pension` 1, `property` 1.
- Holdings: `savings` = 0 (no-op), `loan` = 2, `credit` = 1 (these 3 become `liability`), plus `cash` 23, `stock` 43, `etf` 8, `fund` 2, `property` 1.

So on the real DB this change only rewrites 3 holding rows (loan/credit -> liability). The cash-account and savings-holding migrations are no-ops today but must exist for correctness and other DBs.

### Backend touchpoints

- `backend/src/model.rs`
  - `enum AccountType` (~L306): remove `Cash` variant; remove its `as_str` arm (~L327) and `parse` arm (~L341).
  - `enum HoldingType` (~L859): remove `Savings`, `Loan`, `Credit`; add `Liability`. Update `as_str` (add `Self::Liability => "liability"`) and `parse` (add `"liability" => Some(Self::Liability)`). Enum uses `#[serde(rename_all = "lowercase")]`.
- `backend/src/importers/document_parser.rs`
  - `parse_holding_type_fuzzy` (~L920): change `"loan" | "debt" => HoldingType::Loan` and `"credit" | "credit_card" => HoldingType::Credit` into a single `"loan" | "debt" | "credit" | "credit_card" => HoldingType::Liability`. Consider adding `"liability"` as an accepted input token. Update the unit test (~L965) if you add a liability assertion.
- `backend/src/storage/db.rs`
  - `is_available_account` (~L4355): remove the `| AccountType::Cash` arm.
  - `account_type_to_asset_class` (~L4368): remove `AccountType::Cash` from the `AssetClass::Cash` match arm.
  - `compute_savings_growth` (~L3723): rewrite. Instead of filtering `r.holding.holding_type == HoldingType::Savings`, filter `matches!(r.account_type, AccountType::Savings | AccountType::EmergencyFund)` and sum FX-converted `r.holding.value`; return `sum(end) - sum(start)`. Mirror the `sum_carry_forward` closure used by `compute_investment_metrics` (~L3443), which already filters by `account_type`. `get_holdings_for_summary` rows carry `account_type`.
  - `migrate_schema` (~L4820): add three idempotent statements (near the other `ALTER`/data steps):
    - `UPDATE accounts SET type = 'checking' WHERE type = 'cash'`
    - `UPDATE holdings SET holding_type = 'cash' WHERE holding_type = 'savings'`
    - `UPDATE holdings SET holding_type = 'liability' WHERE holding_type IN ('loan', 'credit')`
    - Confirm `accounts.type` and `holdings.holding_type` are plain `TEXT` with no CHECK constraint that would reject the update (schema shows `holding_type TEXT NOT NULL DEFAULT 'stock'`, no CHECK; verify `accounts.type` likewise).
- `db/sql/schema.sql`
  - `accounts.type` comment: drop `cash` from the enumerated-values comment (a prior change already lists `emergency_fund`/`property`).
  - `holdings.holding_type` comment (~L106, L112): reflect the new allowed set (`...cash`, `property`, `liability`; no `savings`/`loan`/`credit`). The `_CASH` cash-balance convention is unchanged (`cash` holding type stays).

### Frontend touchpoints

The TS `AccountType`/`HoldingType` unions are ts-rs generated (`frontend/src/bindings/AccountType.ts`, `HoldingType.ts`, re-exported from `frontend/src/types`). Do NOT hand-edit them; they regenerate on `cargo test` and must be committed. Once regenerated, `tsc` will flag every `Record<AccountType, ...>` / `Record<HoldingType, ...>` that still lists a removed key, which points you at all of these:

- `frontend/src/lib/colors.ts`: `ACCOUNT_TYPE_COLORS` and `ACCOUNT_TYPE_LABELS` - remove the `cash` entries (~L10, L22).
- `frontend/src/lib/account_type_colors.ts`: `PALETTE` - remove the `cash` entry (~L10).
- `frontend/src/pages/settings/accounts_section.tsx`: `ACCOUNT_TYPE_TAGS` remove `{ kind: "single", type: "cash", label: "Cash" }` (~L40); `ACCOUNT_TYPE_HELP` remove the `["cash", ...]` row (~L52).
- `frontend/src/pages/portfolio/investments_detail.tsx`: `HOLDING_TYPE_COLORS` and `HOLDING_TYPE_LABELS` (`Record<HoldingType, ...>`) - remove `savings`/`loan`/`credit` (~L32-35, L45-48) and add a `liability` entry to each. `INVESTMENT_HOLDING_TYPES` (~L58, stock/etf/fund/bond/crypto) is unaffected.
- `frontend/src/pages/portfolio/portfolio_overview.tsx`: the Net Savings tooltip copy (~L269, currently "Checking & savings accounts") should describe the new definition (savings + emergency-fund account balance change over the range).
- Mock data / mock service (mock mode powers Vercel): audit and fix any removed type strings in `frontend/src/data/mock_holdings.ts`, `mock_accounts.ts`, `mock_account_balances.ts`, and `frontend/src/api/mock_service.ts`. Grep for `"cash"` account types and `"savings"`/`"loan"`/`"credit"` holding types.

### Docs / API

- `docs/api.html`: grep for any enumerated type values in endpoint descriptions (accounts, holdings, cash-summary) and update. The api-docs parity check (`python scripts/check_api_docs.py`) is method+path+param based, so pure value-list wording will not fail it, but keep it accurate.
- No routes or params change here, so `check_api_docs.py` should stay green regardless.

### Verification

- Backend: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (regenerates + commits `bindings/AccountType.ts` and `HoldingType.ts`), `python scripts/check_api_docs.py`.
- Frontend: `cd frontend && npx tsc -b && npm run build`.
- Manual (real DB, via API only): restart backend on `X:/projects/fynance/data/fynance.db`, then
  - `GET /api/accounts` shows no `cash` type.
  - `GET /api/holdings?account_id=...` per account shows no `savings`/`loan`/`credit`; the former loan/credit rows are now `liability`.
  - `GET /api/budget/cash-summary?start=&end=` returns a `savings_growth` that reflects the balance change of the 3 savings accounts over the range (no longer forced to 0).
- Frontend smoke: check the Add/Edit Account type picker no longer offers Cash; the portfolio Investments breakdown labels liabilities as "Liability"; Net Savings tile is populated.

---

## Part B - Test sign-off for shipped features (items 10-17)

These were built earlier this session and are on this branch already. Numbers are frozen (do not renumber). Items 1-9 are already signed off. Drive the UI at `:5173` (real data) and confirm each. Screenshots go to `.playwright-mcp/`.

| # | What to verify | Where it lives |
|---|---|---|
| 10 | Filter row: dropdown width and muted placeholder render correctly | shared MultiSelect + filter row in `frontend/src/pages/budget.tsx` |
| 11 | Category-type filter ordering is correct and `internal_transfer` is always last | `frontend/src/lib/category_types.ts` (`CATEGORY_TYPE_GROUPS` order) |
| 12 | Spending Trends: periods with no data render as gaps, not phantom £0 (a real 0 still shows 0) | `frontend/src/components/charts/styled_line_chart.tsx` + backend series emit null for no-data |
| 13 | Chart type colors are correct when Group by = Category type | `frontend/src/lib/category_types.ts` (`colorForType`/`colorForGroupLabel`) used by the budget charts |
| 14 | Cumulative-invested chart shows two lines: cumulative invested + market value of investment/ISA holdings | portfolio cumulative-invested chart (`frontend/src/pages/portfolio/`), backed by `GET /api/holdings/history` and investment events |
| 15 | Accounts view balances are shown as of the range's end date | Accounts list / `GET /api/holdings/balances` (carry-forward to end date) |
| 16 | Chart right-click drill-downs, each refining the current view: a) bar -> transactions, b) line -> transactions, c) pie -> transactions, d) Portfolio History -> Accounts with the clicked date range | `frontend/src/components/charts/chart_context_menu.tsx`, `frontend/src/pages/budget/chart_drill.ts`, `budget_stacked_bar.tsx` / `budget_line_chart.tsx` / `budget_pie_chart.tsx`, `frontend/src/pages/portfolio/portfolio_history.tsx` |
| 17 | Transactions: a) multi-select, b) shift-click range select/unselect, c) bulk set-category, d) single delete, e) bulk delete. Bulk-action bar sits in the filter row (checking the first row must not shift the table) | `frontend/src/pages/transactions/transaction_table.tsx` (controlled selection, `BulkCategoryPicker`), selection state + bulk bar in `frontend/src/pages/budget.tsx` |

As items are confirmed, tick them off. If a test uncovers a bug, extend `frontend/scripts/smoke_preview.mjs` so it is caught automatically going forward.
