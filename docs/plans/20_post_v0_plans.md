# Future Plans

Post-V0 improvements grouped by urgency. Items copied here from the V0 burndown are marked with version tags. Items that originated from earlier closed plans note their source.

---

## V1 (Immediate next steps after V0)

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

### Document Import Enhancements

- [ ] Support image uploads / screenshots (same import flow as CSV/PDF, extraction handled by the LLM) (from V0 burndown, marked V1)
- [ ] Support multiple files per single account in one import call, with the LLM having context across all files for that account (useful for multiple screenshots) (from V0 burndown, marked V1)

### CORS

- [ ] Tighten CORS from `CorsLayer::permissive()` to explicit `http://127.0.0.1:<port>` and `http://localhost:<port>` origins (from `17_frontend_review.md`)

---

## V2

### Display Currency

- [ ] Allow user to set a preferred display currency (stored in FE, maybe also in BE profile)
- [ ] All monetary values in UI show converted amount with source amount as tooltip
- [ ] Conversion uses exchange rates for historical accuracy (from `13_frontend_backend_handover_unimplemented.md` Section 3.3)

---

## V3

### Rules-Based Categorization

- [ ] Develop rules-based per-sender category assignment as a fallback or complement to AI categorization (from V0 burndown shared questions)
- [ ] A rule is: "all transactions to/from this sender go to this category"


---

## V4

### Exchange Rates Table (FE)

- [ ] Frontend currency conversion at display time: load exchange rates for a given date and convert holdings/transactions to preferred currency
- [ ] Note: holdings snapshots already capture value + currency at snapshot date, so historical values are preserved. This is purely a display-time convenience for multi-currency views. (from `13_frontend_backend_handover_unimplemented.md` Section 3.1)

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
