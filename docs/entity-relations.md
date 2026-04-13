# Entity Relationship Diagram

Visual overview of all data models in the fynance API.

**Color coding:**

- 🟢 **Green** — Agreed and shipped on `master`
- 🟡 **Yellow** — Proposed / pending decision (from handover, plan #13, or Appendix B)

Source of truth for shipped entities: [db/sql/schema.sql](../db/sql/schema.sql) and [db/sql/migrations/](../db/sql/migrations/).

### 🟢 Shipped entities

```mermaid
erDiagram
    profiles {
        TEXT id PK
        TEXT name
    }
    accounts {
        TEXT id PK
        TEXT name
        TEXT institution
        TEXT type "checking/savings/investment/credit/cash/pension"
        TEXT currency
        TEXT balance "Decimal"
        TEXT balance_date
        INTEGER is_active
        TEXT notes
        JSON profile_ids FK "→ profiles.id[]"
    }
    transactions {
        TEXT id PK "UUID v4"
        TEXT date "ISO datetime"
        TEXT description
        TEXT normalized
        TEXT amount "Decimal, neg = debit"
        TEXT currency
        TEXT account_id FK
        TEXT category "Parent: Child"
        TEXT category_source "rule/agent/manual"
        REAL confidence
        TEXT notes
        INTEGER is_recurring
        TEXT fingerprint UK "SHA-256 dedup"
        TEXT fitid
        TEXT created_at
    }
    holdings {
        INTEGER id PK
        TEXT account_id FK
        TEXT symbol "or _CASH"
        TEXT name
        TEXT short_name
        TEXT holding_type "stock/etf/fund/bond/crypto/cash"
        TEXT quantity "Decimal"
        TEXT price_per_unit "Decimal"
        TEXT value "Decimal"
        TEXT currency
        TEXT as_of
        TEXT created_at
    }
    standing_budgets {
        INTEGER id PK
        TEXT category UK
        TEXT amount "Decimal, monthly target"
    }
    budget_overrides {
        INTEGER id PK
        TEXT month "YYYY-MM"
        TEXT category
        TEXT amount "Decimal"
    }
    section_mappings {
        TEXT section "Income/Bills/Spending/Irregular/Transfers"
        TEXT category UK
    }
    import_log {
        INTEGER id PK
        TEXT filename
        TEXT account_id FK
        INTEGER rows_total
        INTEGER rows_inserted
        INTEGER rows_duplicate
        TEXT source
        TEXT detected_bank
        REAL detection_confidence
        TEXT imported_at
    }
    ingestion_checklist {
        INTEGER id PK
        TEXT month "YYYY-MM"
        TEXT account_id FK
        TEXT status "pending/completed/skipped"
        TEXT completed_at
        TEXT notes
    }
    api_tokens {
        INTEGER id PK
        TEXT name UK
        TEXT token_hash "SHA-256"
        TEXT created_at
        TEXT last_used
        INTEGER is_active
    }
    profiles       ||--o{ accounts            : "profile_ids[]"
    accounts       ||--o{ transactions        : "account_id"
    accounts       ||--o{ holdings            : "account_id"
    accounts       ||--o{ import_log          : "account_id"
    accounts       ||--o{ ingestion_checklist : "account_id"
    standing_budgets ||--o{ budget_overrides  : "category"
    section_mappings }o--|| standing_budgets  : "category"
```

### 🟡 Pending and proposed entities

Separate diagram so the pending items stack below the shipped core instead of expanding it sideways. Grey `(existing)` boxes are references to tables defined in the shipped diagram above; the yellow items are new.

```mermaid
erDiagram
    exchange_rates {
        INTEGER id PK
        TEXT from_currency
        TEXT to_currency
        TEXT rate "Decimal"
        TEXT rate_date
        TEXT source "manual/api"
        TEXT captured_at
    }
    transaction_edits {
        INTEGER id PK
        TEXT transaction_id FK
        TEXT field "category/notes"
        TEXT old_value
        TEXT new_value
        TEXT changed_at
        TEXT changed_by "user/agent"
    }
    transactions_existing {
        TEXT id PK "see shipped diagram"
    }
    holdings_existing {
        INTEGER id PK "see shipped diagram"
    }

    transactions_existing ||--o{ transaction_edits : "transaction_id"
    holdings_existing     }o--o{ exchange_rates    : "currency + as_of"
```

Schema-level refinements (no new tables, so not drawn above) — see Status Legend below for details:

- **holdings pot support** — drop fixed `_CASH` sentinel or widen `UNIQUE(account_id, symbol, as_of)` so a single account can carry multiple cash sub-balances (pots, multi-currency)
- **accounts.type** — add `property` and `mortgage` to the `AccountType` enum
- **transactions.date: datetime-always** — stop zero-padding date-only imports to `T00:00:00`; synthesize distinct per-row times at import
- **timezone policy** — stamp user's local timezone at ingestion so stored values are stable across devices

## Status Legend

### 🟢 Shipped and agreed

| Entity | Notes |
|--------|-------|
| **profiles** | Multi-person household support. Seeded with a "default" row on first startup. |
| **accounts** | Core entity. `profile_ids` is a JSON array (not a join table). Types: `checking`, `savings`, `investment`, `credit`, `cash`, `pension`. |
| **transactions** | Deduplicated by `fingerprint = sha256(date \| amount \| account_id)`. `is_recurring` flag for budget projections. |
| **holdings** | Per-symbol detail within accounts, plus cash balances as `symbol='_CASH'`, `holding_type='cash'`. Migration 004 consolidated `portfolio_snapshots` into this table (Option A of the consolidation proposal). `short_name` added for chart labels. |
| **standing_budgets + budget_overrides** | Option C from the handover: standing targets with per-month overrides. `COALESCE(override.amount, standing.amount)` gives the effective value. |
| **section_mappings** | Maps categories to spending grid sections (Income, Bills, Spending, Irregular, Transfers). |
| **import_log** | Audit trail per import. Migration 001 added `detected_bank` and `detection_confidence`. |
| **ingestion_checklist** | Monthly progress tracker: "3 of 7 accounts updated for March". |
| **api_tokens** | Bearer tokens for programmatic/agent access. Hash-only storage. |
| **budgets** (legacy) | Old per-month table. Superseded by `standing_budgets` + `budget_overrides` but not yet dropped. |

### 🟡 Pending / proposed

| Item | Source | Status |
|------|--------|--------|
| **exchange_rates** | Plan 13 §3, handover §Currency | Not shipped. Needed for multi-currency net worth with historical accuracy. |
| **transaction_edits** (audit log) | Appendix B.4 | Not shipped. `PATCH /api/transactions/:id` currently overwrites silently. Minimum ask: one row per changed field with old→new for revertability. |
| **holdings pots support** | Plan 13 §1 | Not shipped. `UNIQUE(account_id, symbol, as_of)` + fixed `_CASH` sentinel blocks Monzo pots and multi-currency balances. Pick Option A (meaningful symbols), B (widen UNIQUE), or C (new column). |
| **accounts.type: property, mortgage** | Handover §1.4 | Not shipped. Frontend already uses these; backend enum needs extending or frontend needs to stick to existing types. |
| **transactions.date: always datetime** | Appendix B.6 | Not fully shipped. CSVs with date-only dates still zero to `T00:00:00`, causing fingerprint collisions on same-day same-amount rows. Importer must synthesize distinct times per row. |
| **timezone policy** | Appendix B.2 | Not shipped. Naive timestamps through the whole stack. Proposal: stamp user's local TZ at ingestion time. |

### Derived views (not stored)

These API response shapes are built from the entities above, not separate tables:

| View | Built from |
|------|-----------|
| **PortfolioResponse** (net worth, breakdowns by type/institution/sector) | accounts + holdings |
| **PortfolioHistoryRow** (available vs unavailable wealth per month) | holdings (with carry-forward) |
| **BudgetRow** (budgeted vs actual per category) | standing_budgets + budget_overrides + transactions |
| **SpendingGridRow** (category × period pivot) | transactions + section_mappings + standing_budgets |
| **CashFlowMonth** (income vs spending per month) | transactions |
| **CategoryTotal** (spend per category) | transactions |
| **BalanceDelta** (first/last balance per account in a range, via `?summary=true`) | holdings |
