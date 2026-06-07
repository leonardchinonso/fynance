# Plan 23: Capital Gains Tax — Post-V0

**Date:** 2026-06-07 (updated)
**Status:** Backend engine + V0 finishing work shipped; report UI in flight; HMRC-grade items tracked below
**Target version:** rolling, post-V0
**Supersedes:** Implementation portions of [`21_capital_gains_tax.md`](archive/21_capital_gains_tax.md). Plan 21 (now archived) remains the design rationale and the HMRC background reference.

---

## 1. Scope

This is the rolling tracker for the Capital Gains Tax feature, from the engine shipped in PR #59 through to the long-term goal of replacing a UK accountant entirely.

The CGT work doesn't fit a single version number — different pieces will ship in V1, V2, V3+ depending on how much HMRC-grade output the user wants in any given release. This doc tracks every piece, what's done, and what's next.

---

## 2. Shipped

### Backend engine (PR #59)

- [`backend/src/server/routes/capital_gains.rs`](../../backend/src/server/routes/capital_gains.rs) — implements same-day FIFO, 30-day Bed & Breakfast, S104 pool replay, and unmatched-disposal remainder, with `tax_year` / `start_date`–`end_date` / `as_at` filtering and ISA/Pension exclusion via `accounts.account_type`.
- [`backend/tests/capital_gains.rs`](../../backend/tests/capital_gains.rs) — integration test covering each matching rule independently, ISA exclusion, and the tax-year query.
- Endpoints registered in [`server/mod.rs`](../../backend/src/server/mod.rs):
  - `GET /api/investments/pools?as_at=` — S104 pool snapshot
  - `GET /api/investments/capital-gains` — full CGT report

### V0 finishing work (PR #63)

- ts-rs bindings (`CapitalGainsResponse`, `CgtSummary`, `SymbolSummary`, `CgtRealizedEvent`, `CgtMatchDetail`, `S104PoolState`) regenerated and committed.
- Design doc at [`docs/design/08_cgt_engine.md`](../design/08_cgt_engine.md) covering inputs, algorithm, API, storage model, and limitations.
- [`CLAUDE.md`](../../CLAUDE.md) REST API surface updated.
- OpenAPI spec at [`backend/src/server/routes/docs.rs`](../../backend/src/server/routes/docs.rs) extended with both endpoints and full response schemas.

---

## 3. In flight: report UI (V1)

Per the plan at `C:\Users\opemi\.claude\plans\fluttering-meandering-tower.md`:

- Reports landing with a two-card layout (CGT + a "more reports coming soon" placeholder).
- CGT report page at `/reports/cgt` with profile + period filters.
- On-screen report styled after the user's actual SA100 filing — Capital Gains Disposal Summary supplementary pages.
- Generated reports persist in localStorage and have their own URL (`/reports/cgt/:reportId`); a history list shows recent generations.
- Generate PDF button using `@react-pdf/renderer`, client-side.
- Backend ask: add `profile_ids` CSV filter to both CGT endpoints so the engine can scope to a profile set while keeping the S104 pool global per symbol.

---

## 4. Post-V0 work — path to "replace the accountant"

These items are what the user's SA100 filing actually contains beyond what the engine produces today. None of them are blockers for V1; all are needed before the report is filing-grade unaided.

### Historical FX rates per disposal date

Required for HMRC-grade reporting on non-GBP trades. HMRC requires each leg to be converted at the date-specific rate: acquisition leg at acquisition-date rate, disposal leg at disposal-date rate. You may not convert the gain itself.

Today the engine applies the current `currencies` rate to every event regardless of date. GBP-only portfolios are unaffected; USD/EUR/etc trades will show converted numbers that don't match the filed figures.

Shared with multi-currency plan §V4 (date-keyed `exchange_rates` table, provider integration). Implementation: extend `FxRateMap` with a `convert_as_of(amount, currency, date)` method backed by the cache; pass acquisition/disposal dates into the conversion calls in `run_cgt_engine`.

### Annual Exempt Amount (AEA)

Subtract the year's AEA from total gains before tax is computed. Tax-year-specific value:

| Tax year | AEA |
|----------|-----|
| 2023/24  | £6,000 |
| 2024/25  | £3,000 |
| 2025/26  | £3,000 |

User-editable in case HMRC changes it mid-year. UI: a small "Reliefs" panel above the summary card that lets the user toggle AEA on/off and override the default amount.

### Brought-forward losses

Track unused capital losses from prior tax years. Let the user enter an opening balance per profile per tax year; the engine carries forward each year automatically. Pulled into the chargeable-gain calculation alongside the AEA.

The user's filed SA100 already has a "Capital Losses Summary" page showing this. Mirror the layout.

### BADR / Investors' Relief

Per-disposal flag and reduced rate (currently 10% for BADR, 14% from 6 Apr 2025, 18% from 6 Apr 2026). UI: a checkbox on each row of the disposal schedule to mark "BADR claimed". Total relief-qualifying gains roll into the tax computation at the reduced rate.

### Mid-year rate-split (2024/25 specifically)

UK CGT main rates increased on 30 Oct 2024: 10%/20% → 18%/24%. The SA108 has a "Capital Gains Adjustment Summary" page that splits disposals into pre/post-30-Oct buckets and applies different rates to each. Year-specific quirk but a real one — needed for any actual filing covering 2024/25.

Implementation: bucket `realized_events` by `disposal_date` against 2024-10-30, apply per-bucket rates from a config table. The config table also makes future rate changes a config edit rather than a code change.

### CSV and SA108 form-field export

PDF lands in V1; CSV (one row per disposal with the engine's full per-row breakdown) and a structured **SA108 box-by-box export** (so the user can paste each box into HMRC's online filing) are follow-ups. The box mapping is approximately:

| SA108 box | Engine field |
|-----------|--------------|
| 23 / 24 / 25 / 26 (Listed shares — disposals, proceeds, costs, gains) | aggregates over `realized_events` |
| 31 / 32 / 33 (unlisted shares) | not currently distinguished — needs an instrument-type tag |
| 47 (losses claimed) | sum of negative `realized_events` |
| 50.1 (BADR lifetime allowance) | future BADR feature |

### Reverse splits and share consolidations

The `Split` branch in `run_cgt_engine` scales `pool_shares` by `quantity`. Forward splits with `quantity > 1` work; consolidations with `quantity < 1` are untested and need an explicit test case before production use.

### Unmatched-disposal UX

Disposals that exhaust all three matching rules are emitted with `cost_basis = 0` and `rule_applied = "Unmatched"`. The frontend already plans to surface these with an amber row-highlight + tooltip. Beyond that, the report could refuse to PDF until every unmatched row is resolved (either by adding a missing acquisition event or by explicitly confirming the disposal is a genuine short sale).

---

## 5. End goal

The Capital Gains Tax report should be sufficient to file with HMRC unaided. That means:

- Every figure in the user's SA108 supplementary pages can be sourced from the report.
- Every "Capital Gains Disposal Summary" workings page (one per matched bucket) is generated automatically.
- The Capital Losses Summary, Intermediary Summary, and Adjustment Summary pages are produced when applicable.
- The user can submit the result as-is, or hand it to an accountant for a sanity check rather than the full computation.
