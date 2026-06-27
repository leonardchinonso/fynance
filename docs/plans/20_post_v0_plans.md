# Future Plans

Post-V0 improvements grouped by urgency. Items copied here from the V0 burndown are marked with version tags. Items that originated from earlier closed plans note their source.

---

## V1 (Immediate next steps after V0)

### Features
- [ ] **Custom tags/labels on accounts:** Add a `tags: string[]` field to `accounts` for user-defined free-form labels (e.g. "locked-until-2027", "joint-expenses", "rewards-card"). Sits alongside the fixed `account_type` enum and `profile_ids` array to allow softer categorisation that doesn't fit a type. Surfaced as filter chips on the accounts grid and as optional groupings in summary breakdowns. Originally raised in `archive/18_project_brief.md` Open Questions.
- [ ] **ISA allowance tracking:** Per-account annual allowance cap (e.g. £20,000 for a Stocks ISA). Track how much has been deposited in the current tax year vs. the limit. Show a progress bar + remaining allowance in the account detail sheet. Tax year resets on 6 April. Allowance is per-account-type (S&S ISA, Cash ISA, LISA each have separate rules). UI: a small "ISA" badge on the account card that turns amber/red as the limit approaches.
- [ ] **Mortgage overpayment allowance tracking:** Per-account annual overpayment cap (typically 10% of outstanding balance, but user-configurable). Track total overpayments made in the current mortgage year vs. the cap. Show remaining headroom in the account sheet. Trigger: a holding or account-level `overpayment_limit` field (absolute amount or % of balance, with a year-start date). UI: shown only on accounts of type `mortgage` or where the field is set.
- [ ] **Per-holding custom metadata fields:** A flexible `JSONB`-style (or separate key-value table) `metadata` field on holdings for user-defined annotations. Examples: purchase price / cost basis for CGT tracking, vesting date for RSUs, sector tag, broker reference. API: `PATCH /api/holdings/:account_id/:symbol` extended to accept `metadata: Record<string, string>`. UI: an expandable "Details" row in the holdings sheet showing key-value pairs, editable inline. This is also the natural foundation for the ISA and mortgage features above.
- [ ] **Login page + credential management:** Dedicated `/login` page where users enter their bearer token. Auto-redirect to login on 401 if no token is set in localStorage. V2: multi-user session model with per-user credentials and a server-side revocation UI.
- [ ] **`fynance profile` CLI subcommand (add/list/remove):** Profiles can only be created via the web UI / `POST /api/profiles` today. Now that there is no implicit `default` profile and account creation requires an existing profile, a CLI-only setup on a fresh database has no way to create the first profile, so `fynance account add --profile <id>` fails with nothing to point at. Add `fynance profile add --id <id> --name <name>`, `fynance profile list`, and `fynance profile remove <id>` mirroring the existing profiles REST handlers (reuse `Db::create_profile` / `get_profiles` / `delete_profile`, and the same "in use by accounts" guard as the DELETE route). Context: the removal of the auto-seeded default profile (see the categories-hard-delete branch / profiles fix).
- [ ] **Investments front end (no UI for investment events):** The backend has a full investment-events API (`GET/POST /api/investments`, `PATCH/DELETE /api/investments/:id`, `/api/investments/pools`, `/api/investments/capital-gains`) and these events drive the CGT report and S104 pools, but there is **no front-end view for the events themselves**. Today the UI only shows derived holdings snapshots under the Portfolio tab; the underlying buy/sell/vest/dividend events are invisible and uneditable in the browser. Add an Investments view: a table of events (filter by account / symbol / event_type), inline add/edit/delete, and a link from a holding to its contributing events. This is also where the new per-row `source_document_ids` "Source" column for investments will live. Tracked in [issue #68](https://github.com/leonardchinonso/fynance/issues/68).

### Category Model: remove hardcoded category references

Several backend behaviours key off category *display names* hardcoded in Rust, instead of off the category definition itself. `config/categories.yaml` is the legitimate seed source of truth, and `VALID_SECTIONS` in `sections.rs` is fine (it is the fixed section taxonomy, not categories). The problem is the places below that re-encode category meaning outside the category definition, especially now that categories are user-editable and hard-deletable. The theme: properties of a category (which section it belongs to, whether it is income, whether it is the investment-transfer bucket) should be declared *on the category* (config/flags/columns), not matched by name at runtime.

- [ ] **`PARENT_SECTION_MAP` should live on the category definition.** `PARENT_SECTION_MAP` in `db.rs` hardcodes each top-level parent name to a section (`Income -> Income`, `Finance -> Transfers`, etc.) and is a second copy of the parent names already in `categories.yaml`, so the two can drift. Instead, a category should declare its section as part of its definition (e.g. a `section` field in `categories.yaml` per parent, seeded into `section_mappings`), rather than the mapping being hardcoded in a separate const. After that the const goes away and the section is just another category property.
- [ ] **`compute_investment_metrics` should not match a hardcoded category name.** `new_cash_invested` is currently the signed sum of transactions whose category name equals `Investment Transfer` / `Finance: Investment Transfer` ([db.rs](../../backend/src/storage/db.rs) `compute_investment_metrics`). This silently breaks to `0` if that category is renamed or hard-deleted, and then `market_growth` wrongly absorbs all contributions. Derive it dynamically instead: cash genuinely entering an investment account. Investigate keying off the investment event `type` (`InvestmentEventType`: `vest`, `buy`, `sell`, `transfer`, `withhold`, `split`) and the account being an investment account, so "new cash in" comes from the structured event data rather than a free-text category name. Worst case, resolve the category via a stable key/flag rather than its display name.
- [ ] **"Is income" should be a flag on the category, not a name/section convention.** Income is currently inferred indirectly (sign-based aggregation, plus the `Income -> Income` section mapping seeded from the hardcoded parent name). Add an explicit `is_income` flag settable on a category, and have the income views read that flag. This removes the implicit coupling to a category literally named "Income" and lets users define their own income categories.

### CI/CD and Release Pipeline

- [ ] `ci.yml`: fmt, clippy, test, frontend build + typecheck (from `12_frontend_backend_consolidation.md` Phase 6.5)
- [ ] `docker.yml`: build and push to GHCR on push to main (from Phase 6.5)
- [ ] Block direct pushes to main; create a feature branch -> develop (staging) -> main (release) pipeline
- [ ] Pushing to main automatically deploys a new Docker version to the registry
- [ ] Release branches tracking past releases to allow patching; consider develop tracking RC releases too
- [ ] Investigate how much of the above is available on free public GitHub repos
- [ ] Update Vercel to auto-deploy on push to master
- [ ] Move the live demo link to the top of the README and make it a button
- [ ] Vercel deployment always uses mock data; everything else defaults to live data (configurable via optional `MOCK_ONLY` env var, see Settings page below)

### Testing

- [ ] Add frontend tests (component + integration)
- [ ] Add backend tests (unit + integration)

### Efficiency

- [ ] Verify frontend offloads as much computation to the backend as possible (spending grid, chart aggregation, portfolio summaries should all be server-computed)

### Reports and Export

- [ ] `GET /api/reports/:month`: monthly summary (total income, total spending, net savings, top categories, top merchants, month-over-month deltas) (from `12_frontend_backend_consolidation.md` Phase 5.1)
- [ ] Frontend: Reports page wired to real API, summary cards, category breakdown, top merchants, MoM deltas, export button (from Phase 5.4)
- [ ] `GET /api/export?year=YYYY&format=csv`: full-year transaction CSV export (from Phase 5.2)
- [ ] `GET /api/export?month=YYYY-MM&format=md`: single-month Obsidian-compatible markdown (from Phase 5.2)
- [ ] `GET /api/export?year=YYYY&format=md`: full-year markdown (from Phase 5.2)
- [ ] Per-view/table export from the UI: optionally export the current table or chart (CSV / Markdown / image). A non-functional Export dropdown (CSV / Image / Markdown) was removed from the Transactions / Budget / Portfolio headers (PR #86); reintroduce it once it is backed by a real export. Part of Batch 6.

### Document Import Enhancements

- [x] Support image uploads / screenshots — **done (PR #72)**: `format_detection` recognizes PNG/JPEG/GIF/WEBP (by extension and magic bytes) and routes them to the LLM as image content blocks, same import flow as PDF.
- [ ] Cross-file LLM context for multi-file imports. Multi-file **upload** + parallel per-file extraction is done; this is the narrower case of feeding all of an account's files into a single prompt so the model can reason across them (e.g. stitching one statement split across screenshots). Lower priority — parallel-independent extraction is the better default for separate statements.
- [ ] Fingerprint collision disambiguation. The dedup hash is `sha256(date, amount, account_id)`, so two genuinely distinct same-day / same-amount transactions on one account collapse (the second is dropped as a duplicate). Add a deterministic tiebreaker (e.g. `duplicate_index`) that stays idempotent on statement re-import. (from V0 burndown, deferred)
- [ ] Consolidate parse-hint validation into one place. `ReturnType::is_valid()` lives on the type but the `holdings.period` requires `transactions` rule is enforced separately in the `POST /api/parse` route handler, so validity is split across layers (a `ReturnType` can pass `is_valid()` yet be rejected by the route). Move all of it onto the type (e.g. a single `ReturnType::validate() -> Result<(), _>`) so callers cannot construct/accept an invalid config. (from PR #50 review)

### CORS

- [ ] Tighten CORS from `CorsLayer::permissive()` to explicit `http://127.0.0.1:<port>` and `http://localhost:<port>` origins (from `17_frontend_review.md`)

### Bugs
- [ ] Mouse scrolling really quickly on the recharts pie chart sometiems fails to trigger the onMouseLeave event
  - Root cause: Recharts maintains its own internal `active` state for the tooltip independently of React state. Our `activeIndex` and `mousePos` state clear correctly, but Recharts' internal state does not, causing the tooltip and active shape to remain visible.
  - Confirmed via logging: when stuck, `activeIndex: undefined, mousePos: null` in React state, but `rechartsTooltip.active: true` from within the `content` render prop.
  - Also observed: `onPointerMove` fires outside the container boundary (pointer capture behaviour), producing negative `mousePos.x` values that keep the tooltip alive even after our state is nominally cleared.
  - Attempts that did not fully resolve it: moving `onMouseLeave` to `<PieChart>` (bounding box vs SVG path), `mouseInsideRef` guard on slice enter, interval-based position check, `pointerleave` + `pointerenter` listeners, `effectiveActiveIndex` derived from `mousePos`.
  - Likely fix direction: pass `active={false}` explicitly to `<Tooltip>` when we want it hidden (controlled mode), which bypasses Recharts' internal state entirely. Not yet attempted.

---

## V2

![alternative history networth visualization](assets/alternative_history_networth_visualization.png)

### Multi-Currency: Automatic Rate Fetching

V0 is purely user-defined rates with a staleness timestamp. V2 adds auto-refresh: on each holdings summary request, if a stored rate is older than the staleness threshold (default: 1 day), the backend fetches a fresh rate from the provider and updates the stored value. Full spec in [docs/plans/22_multi_currency.md](22_multi_currency.md) (V2 section).

Summary:
- Provider: [frankfurter.app](https://frankfurter.app) — free, no API key, ECB data
- Stale rate (older than threshold) triggers a fetch and overwrites `user_fx_rates.rate` + `updated_at`
- Manually pinned rates (`pinned: true` on `user_fx_rates`) are never auto-overwritten
- Cache auto-fetched rates in `exchange_rates` table by `(base, quote, date, source='api')`
- `DELETE /api/fx-rates/cache` to force re-fetch
- `GET /api/fx-rates/resolved` to see active rates with source and `updated_at`
- Include `stale_rates: true` in response if provider unavailable and falling back to cached


### Backups

Frequent automatic snapshots and backups of db so user can't loose months of work at a time

---

## V3

### Rules-Based Categorization

- [ ] Develop rules-based per-sender category assignment as a fallback or complement to AI categorization (from V0 burndown shared questions)
- [ ] A rule is: "all transactions to/from this sender go to this category"

### Multi-Currency: User-Configurable FX Provider

Full spec in [docs/plans/22_multi_currency.md](22_multi_currency.md) (V3 section). Summary:
- [ ] Settings page: FX provider section — URL template and API key input, stored on the profile.
- [ ] Backend: use user-configured provider if set, fall back to frankfurter.
- [ ] Long-term: allow multiple providers with priority order.

---

## V4

### Multi-Currency: Historical Exchange Rates

Full spec in [docs/plans/22_multi_currency.md](22_multi_currency.md) (V4 section). Summary:
- [ ] Use `as_of` date on holdings snapshots to fetch the rate current at snapshot time, not today's rate, for accurate historical net worth chart values.
- [ ] `exchange_rates` table already caches by date — historical lookups query that table, fetching from provider if date is missing.
- [ ] Holdings snapshots already capture value + currency at snapshot date — this is purely about using the right rate per date when aggregating history.
- [ ] **Also needed for accurate per-account / per-holding history, not just the aggregate net worth.** Concrete case: the rebuilt Trading 212 holdings history (from broker confirmation statements) ties to each confirmation's GBP total only within ~1-2%, because `convert_as_of` currently falls back to today's static USD rate for every historical USD snapshot. With per-date rates, a USD holding's GBP value at each past date would match the broker statement exactly. Same root cause as CGT needing the trade-date rate to compute the gain in GBP.

---

## V5

### Document Storage

Creating documents as a first-class primitive. Every import source file is preserved and linked back from the import log and from individual transactions.

#### Storage location

Files are stored on the local filesystem in a `documents/` subdirectory alongside the SQLite database (i.e. `~/.local/share/fynance/documents/` on Linux, equivalent OS data dir on macOS/Windows). This keeps everything self-contained in the same directory the user already backs up for the DB, and avoids SQLite BLOB bloat on large PDFs or images.

Each file is written once and never mutated. Filename on disk: `<import_log_id>_<original_filename>` — the id prefix guarantees uniqueness if the same filename is uploaded twice.

#### Schema changes

Add a `documents` table as a first-class entity:

```sql
CREATE TABLE IF NOT EXISTS documents (
    id          TEXT PRIMARY KEY,           -- UUID
    filename    TEXT NOT NULL,              -- original uploaded filename
    file_path   TEXT NOT NULL UNIQUE,       -- absolute path on disk
    mime_type   TEXT NOT NULL,              -- e.g. text/csv, application/pdf, image/png
    size_bytes  INTEGER NOT NULL,
    uploaded_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);
```

Add `document_id` FK to `import_log`:

```sql
-- add to import_log table
document_id TEXT REFERENCES documents(id)  -- null for imports before V5
```

Add `import_log_id` FK to `transactions` (so any transaction can trace back to its source document via the log):

```sql
-- add to transactions table
import_log_id INTEGER REFERENCES import_log(id)  -- null for transactions before V5
```

#### API

```
GET  /api/documents                    -- list all stored documents
GET  /api/documents/:id                -- document metadata
GET  /api/documents/:id/download       -- stream the file bytes back to the browser
POST /api/documents                    -- upload a standalone document (no import)
DELETE /api/documents/:id              -- delete file from disk and row from DB
GET  /api/import/history               -- import_log rows with document_id joined
```

#### UI

- Documents page: table of all stored files with filename, type, size, upload date, linked account, download button, delete button.
- Transaction row: "Source" icon that links to the originating document's download URL (only shown when `import_log_id` is set).
- Import flow: after a successful import, the stored document appears immediately in the documents list.
- Standalone upload: drag-drop area on the documents page to store files that aren't tied to an import (e.g. PDFs for reference).

#### Notes

- The `documents/` directory should be included in any backup advice surfaced in the UI (alongside the `.db` file).
- If a document is deleted, `import_log.document_id` is set to null but the log row and transactions are preserved.
- No deduplication of file contents at V5 — same bytes uploaded twice creates two document rows. Can revisit with a content hash in V6+ if needed.

---

## V6

### Forecasting
- [ ] Using past trends to predict future spending. i.e. we can use the avg income, avg spending per category, avg savings/investements left over e.t.c to forecast the future spending
    - [ ] On the budget tab this could allow a forecasted view showing values in future date columns, so you can do calculations with dates that haven't happened yet
    - [ ] can be tweaked to play around with scenarios. i.e. "if i drop my eating out to 250 pounds a month how much will that save me after 5 year s what will my acocunt balance be...
- [ ] For this to be truly useful should also take into account non recurruing but guranteed costs
    - [ ] e.g Investements growth can be calculated as an avg of x% pa, where the user can play around with different vlaues of x
    - [ ]  amortized payments such as mortgage should be able to input formulas to figure out how the monthly payment will be split between interest and principal over time.
- [ ] could be used for planning for big purchases like saving for a house, or preparing for lifestyle changes like having a new child.  
- [ ] could maybe also be used for retirement planning, estimatign reduced or no income and seeing how long a portfolio will last spending vs investment growth.  
- [ ] **RSU vesting forecasting (UK tax-aware):** projection endpoint that takes `{gross_qty, vest_price, marginal_rate, employee_ni_rate, employer_ni_passthrough_rate}` and returns the net shares + net cash value after PAYE + employee NI + employer NI passthrough. Useful before a known vest date and as a sanity check on the broker's withhold maths after vest. Needs a user-configured profile for tax / NI rates (doesn't exist yet). Originally raised in [archive/18_project_brief.md](archive/18_project_brief.md) Open Questions.

## Unversioned (Nice-to-Have)

These are ideas worth capturing but not committed to any version.

### Data and Import
- OFX/QIF file import (in addition to CSV)
- Open Banking API integration for automatic transaction pulls
- Receipt photo scanning (OCR) for cash transactions

### AI
- AI chat interface for querying finances ("how much did I spend on travel last quarter?")
- Anomaly detection (flag unusual spending patterns)
- Smart recurring transaction detection (auto-flag transactions that repeat monthly)

### Portfolio
- Real-time stock price fetching for portfolio valuation
- ETF composition drill-down (show underlying holdings)
- Tax-lot tracking for capital gains reporting

### Lifestyle Planning
- Tax planning for capital gains
- Early retirement planning (FIRE calculator)
- Rental income tracking

### Integration
- Obsidian plugin for inline finance queries
- YNAB/Mint import for migration
- Export to common accounting formats

### UI/UX
- Customizable dashboard widgets

### Charting and Visualization
- Click-to-filter: clicking a pie slice to filter the transaction table view
- Synchronized cursors: hovering one chart highlights the same data point on another chart
- Custom animated transitions: chart morphing between view modes
- Candlestick / OHLC charts: if stock price visualization is added
- Waterfall charts: income-to-savings flow visualization
- Sankey diagrams: money flow between accounts
- Brush/zoom on charts: drag to select a time range on a chart to zoom in
