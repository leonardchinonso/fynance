# V0 Burndown

Everything needed to ship a usable V0. Split by owner. These items were pulled from a conversation between Ope and Nonso on 2026-04-18 and reconciled against existing design docs.

---

## Nonso (Backend / API)

### Holdings / Portfolio

- [ ] Rename portfolio endpoint to `/api/holdings` (get rid of all references to `portfolio` as it's confusing, we don't need back compat)
- [ ] Implement importing holding balances from documents, more below
- [ ] Allow multiple cash holdings per account: the current `UNIQUE(account_id, symbol, as_of)` constraint and `_CASH` sentinel blocks Monzo pots and multi-currency balances. Resolve the design question and update schema + API accordingly (tracked in `13_frontend_backend_handover_unimplemented.md` Section 1)
- [ ] Support marking a holding as closed (so it no longer shows in active views but history is preserved)

### Accounts

- [ ] Account `type` field should be an enum: `Savings | Checking | Investment | Pension | Credit | Loan` (confirm final list with Nonso)

### Budget

- [ ] Every category has a budget; auto-carry from previous month unless overridden.
- [ ] Decide where this is stored, per transaction makes no sense, per category or a standalone budget table?


### Categories

Categories move from the hard-coded `categories.yaml` into a proper DB table, user-manageable at runtime. Seed from the YAML on first startup if the table is empty.

**Model:** `id`, `name`, `description` and some notion of a higher-order grouping (e.g. "Food" groups "Groceries", "Dining & Bars", etc.). Grouping is not a category itself.

> **Open question (Nonso):** should the group be a free string on each category row, or its own first-class entity with its own id (allowing future renames)

- [ ] Create `categories` table and seed from `categories.yaml`
- [ ] Bulk upsert, bulk read (ensure to include description this will be useful for assigning categories), bulk delete endpoints
- [ ] Bulk assign categories to transactions ()
- [ ] Maybe add "Budget" as a field on category
  - [ ] This could be a list of Budget snapshot. i.e. on Feeding category we could have `budget: [Jan 2026: 200pounds, April 2026: 250 pounds]`
  - [ ] This should be excluded from bulk read on the categories?

> **Open question (Nonso):** where should budgets live

### Transactions

- [ ] Add an `exclude_from_summary` boolean flag on individual transactions (default false). Used for internal self-transfers (e.g. Monzo to Revolut) that should not distort spending/income summaries. Backend should respect this flag in all aggregation endpoints (spending grid, cash flow, by-category).
> **Open Question for Nonso:** Fingerprint collision disambiguation. For banks that only provide date (no time), same-day same-amount transactions collide. Proposal: allow an optional `duplicate_index` field (integer, default 0) that becomes part of the fingerprint hash. The caller can set this when they know two rows are distinct transactions. Does this work or is there a better approach?

### API: Missing Endpoints

Most entities below need bulk coverage. Single-item endpoints are optional; bulk endpoints are the priority. Create and modify are collapsed into a single bulk upsert — if the record exists it is updated, otherwise it is inserted.

| Entity | Endpoints needed |
|---|---|
| Transactions | Bulk upsert, bulk read, bulk delete |
| Holdings | Bulk upsert, bulk read, bulk delete |
| Categories | Bulk upsert, bulk read, bulk delete, bulk assign to transactions |
| Accounts | Delete account + Update account (or change existing create to upsert) |

> **Open question** "bulk assign to transactions": this is maybe just part of the bulk modify tracactions, where i modify a transaction and as part of the payload I set it's category

Rules:
- Bulk upsert transactions and bulk upsert holdings must support a `dry_run=true` query param (or a `"dry_run": true` request field) that validates and previews the operation without committing, and returns a list of all transactions that would be committed, ()
- Every endpoint must be documented: shape of the request body and shape of the response (see API docs section below)

### Document imports

Currently only CSV is supported. PDFs and images are not. The bulk endpoint (`POST /api/import/bulk`) already accepts multiple files but processes each independently — if you pass two files for the same account the LLM sees them in isolation, there is no cross-file stitching.

- [ ] Support PDF uploads (same import flow as CSV, extraction handled by the LLM)
- [ ] V1: Support image uploads / screenshots (same flow)
- [ ] V1: Support multiple files per single account in one import call, with the LLM having context across all files for that account (useful for multiple screenshots)
- [ ] Add an optional free-form `hints` text field to the import request — user can provide notes, date range context, bank format hints, or anything else that might help the AI parse or categorize correctly
- [ ] Should support dry-run
- [ ] V5: Saving documents. (creating documents as a first class primitive, for each import (csv/pdf/image) preserve the document that led to it as a "source" and each transaction has a "source" button you can click that shows you the csv that lead to it, also, you can have them show up in the documents page in the ui. Also potentially allow just uploading documents that don't even necesarily feed into anything just for central storage.)

##### Trading 212
- [ ] Parse T212 PDF exports, from our scan it has:
  - Opening position for a month/year
  - Trades during the period
- [ ] For now we are skipping extract transactions from T212 data. But do an exploration just to confirm our transaction model is expandable to support this in the future
- [ ] Extract opening balance per period per holding (in the statement)
- [ ] Extract closing balance per period per holding (see if you can derive this from the statement)

##### Monzo
- [ ] Parse monzo csv + pdf (both csv and pdf have unique information that would be useful) from our analysis it had:
  - Categories in the csv, unique ids too
  - Closing balance (after each transaction) on the pdf
  - Balance per pot 
- [ ] Allow uploading multiple documents per account the ai having access to both should allow better resutls
- [ ] Allow extracting multiple closing balances one per pot in the case where the user has set up different pots as different holdings.

#### Holding Snapshots from Imports

- [ ] When importing a CSV/PDF, take a holding snapshot from the **last balance on the file**
- [ ] For files with multiple holdings (e.g. T212, Monzo with multiple pots), generate one holding snapshot per holding from the last transaction for each
- [ ] Holding snapshots go through the same dry-run/confirm flow as transactions

### API Documentation

- [ ] `GET /api/docs` returns an OpenAPI spec that is complete and agent-readable
- [ ] Every endpoint above is documented with: request schema, response schema, field descriptions, and at least one example payload
- [ ] Category taxonomy is included in the docs so external agents know valid values

### Dry run
For both the CSV/PDF/IMAGE imports and be bulk upsert in points where we end up modifying transactions and holdings we should support German which will do the calculation And return a list of all of the modifications that will be made 
- [ ] For the CSV imports this will end up showing every row that would end up being created rules that would end up being modified if there are any conflicts detected in the transactions table and will also show us new snapshots that will end up being created in the holdings It should also include the information about if each of these entries are actually going to end up being a create or a modify in the case of conflicts
- [ ] For bulk upsert api where we already provide the correct shape dry-run will be largely similar to the input but with an additional flag that shows if there was a detected conflict that will be upsert of if this will be a creation.
- [ ] There should be a way to efficiently ack the dry-run. We should NOT be required to hit the import endpoing againg but with dry-run=false this wastes tokens and there's a chance the ai produces different output. Maybe support an endpoint where we can return the dry-run output back to the api to say "commit this"

> Should we just come up with a term for csv/pdf/image, i'm thinking "import" or "document" going forward i will refer to these as "source documents"


### Currency

- [ ] Confirm currency is tracked at the transaction level (schema already has `currency TEXT NOT NULL DEFAULT 'GBP'` on transactions — verify this is wired through to the UI)
- [ ] Currency must also be tracked at the holding level (schema has `currency` on holdings — verify this is surfaced correctly and not dropped anywhere in the pipeline)
- [ ] Currency should likely also be stored on the budget level, basically anywhere a monetary value is stored it should be stored next to a currency (
  - [ ] v:10 maybe monetary value should become a new table entry with money and currency??? and then can be extending to a different type which is like stock + amount of shares held)
- [ ] Amounts are always stored in source currency, never converted at ingestion. Add validation and document convention in code and API docs. (from `13_frontend_backend_handover_unimplemented.md` Section 3.2)

### Type Sharing (ts-rs)

- [ ] Introduce a generic `Paginated<T>` Rust struct with `#[derive(TS)]` so the frontend can drop the hand-written `PaginatedResponse<T>` in `types/api.ts` and import the generated binding instead. Small change, ~20 lines of backend code. (from `13_frontend_backend_handover_unimplemented.md` Section 6.1)


---

## Ope (Frontend / Import / Data Ingestion)


### Data Ingestion UX (Guided Monthly Flow)

- [ ] Sort accounts in a user-defined order for the ingestion wizard
- [ ] Allow accounts to be marked as "never show" (skipped from the ingestion list) this and above stored in the browser.
- [ ] Ingestion wizard flow (browser):
  1. Show next account in the sorted list
  2. Prompt: upload source document (multiple) for this account
  3. Dry run: preview transactions and holdings
  4. Confirm or skip
  5. Advance to the next account
- [ ] This is the browser-side ingestion checklist already tracked in `08_mvp_phases_v2.md` Phase 3 — make sure the account ordering and skip preferences are wired in

### Infrastructure

- [ ] Multi-stage Dockerfile (node build, rust build, debian-slim runtime) + `docker-compose.yml` with volume mount for SQLite (from `12_frontend_backend_consolidation.md` Phase 6.4)

### Settings Page (UI)

A dedicated settings page for CRUD operations and app configuration.

**Profiles:**
- [ ] Create / modify / delete profiles

**Accounts:**
- [ ] Create / modify / delete accounts
- [ ] Create / name holdings within accounts (separate from holding snapshots which are generated on imports). Question: is manual holding creation needed, or do we only get holdings from imports?
- [ ] Sort accounts for the ingestion flow (sort order stored in browser localStorage)
- [ ] Mark accounts as "hidden from ingestion flow" (stored in browser localStorage)

**Categories and Category Groups:**
- [ ] Create / modify / delete categories with descriptions
- [ ] Create / modify / delete category groups
- [ ] Budgets are set in the Budget view, not here

**Data Source Toggle:**
- [ ] Toggle between mock data and live data (live by default)
- [ ] If `MOCK_ONLY` is set in the environment, default to mock and disable the toggle
- [ ] Toggle value stored in browser localStorage so it persists across refreshes (ignored if `MOCK_ONLY` is set)

**Appearance:**
- [ ] Toggle light / dark mode / system default

### Transactions (UI)

- [ ] When BE adds the `exclude_from_summary` flag, expose it in the transaction detail/edit view so users can toggle it per transaction (e.g. for internal self-transfers)

### Budget (UI)

- [ ] Every month has a budget; auto-carry from previous month unless overridden
- [ ] Budget column in the spending table shows the **average** spend for that category (in the selected view range)
- [ ] Hovering a cell in the budget table shows the cell's budget value as a tooltip
- [ ] Add a toggle to show empty categories (stored in browser localStorage): either show only categories with transactions in the selected period, or show all categories even when rows are blank

---

## Shared / Open Questions
-  If AI categorization fails or is unreliable: fall back to rules-based per-sender category assignment.
   -  A rule is basically, 'all transactions to/from this sender should go to this category'
   -  We should maybe develop rules anyway as a v3 feature even if the ai thing works?
-   T212 CSV: can closing positions be reliably derived from opening + trades? or maybe we append screenshots of current holdings as additional imports to the account

