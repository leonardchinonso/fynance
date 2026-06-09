# Plan 23: Capital Gains Tax — Post-V0

**Date:** 2026-06-09 (updated)
**Status:** Backend engine + V0 finishing work shipped; report UI in flight; design-review notes for the next API revision in §6–§7
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

### UTR capture and report attribution

The user's HMRC Unique Taxpayer Reference is the primary identity field on every SA108 page (e.g. `UTR: 2098199578` in the user's filed return). Capture it once at the user/profile level, then include it on the generated PDF's running footer alongside the existing `Generated by fynance · {tax year} · {generated_at} · Page X of Y`.

Implementation sketch:
- Add `utr: Option<String>` to the `profiles` table (NULLable; existing rows unaffected).
- Surface it as a small "Tax identity" panel in Settings → Profiles where the user can paste the 10-digit UTR.
- The PDF footer reads it from the selected profile at generation time and renders `UTR: NNNNNNNNNN` when present, omits the line when not.
- Same field is later consumed by the SA108 box-by-box export (it's box 2 on the SA108 cover).

Out of scope for this slice: an IRMark / submission checksum. That only matters once we actually file with HMRC.

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

---

## 6. Patches landed during the UI build

Issues we hit while building the V1 report UI on top of the engine, and the workarounds that shipped. Documented here so the design review starts from accurate ground state.

### 6.1 `FxRateMap::convert` no longer panics on missing currency

**Symptom:** real-data DB had GBX-denominated events, GBX wasn't in the `currencies` table, the `unwrap_or_else(|| panic!(...))` in `backend/src/util/fx.rs` fired on a tokio worker holding the `Mutex<Db>`. The mutex went `PoisonError` and every subsequent request 500'd until the server was restarted. Frontend saw `502 Bad Gateway`.

**Patch:** `convert` now logs a `tracing::warn!` and returns the amount unchanged when the source currency is missing. Defence-in-depth only — the engine no longer brings the server down for a single bad row.

### 6.2 CGT engine pre-validates required currencies → structured `400 missing_currencies`

**Symptom:** even with 6.1, silently using an unconverted amount produces nonsense totals. The user couldn't tell the report was wrong.

**Patch:** new `check_required_currencies` helper in `capital_gains.rs` collects the distinct set of currencies referenced by in-scope investment events, checks each is present in `FxRateMap`, and returns `AppError::bad_request` with code `"missing_currencies"` and a human-readable list ("Some investment events use currencies not yet configured: GBX. Add them under Settings → Currencies before generating this report."). Frontend matches on the `code` and shows an amber CTA card linking to Settings.

Two CGT tests added (`test_cgt_missing_currency_returns_400`, `test_cgt_profile_ids_no_match`).

**Gap that remains:** investment-event ingest (`POST /api/investments/import`) still has no `validate_currency` call, so the data can keep arriving in unknown currencies. The precheck catches it at read time but the right fix is at write time. Tracked as an open question in §7.1 — depends on which currency-handling direction we pick.

### 6.3 `profile_ids` filter added to CGT endpoints (CSV)

**Symptom:** S104 pool is global per symbol across all included accounts. Calling the per-account API N times and merging client-side would produce wrong pool math.

**Patch:** both CGT endpoints accept `profile_ids` as a comma-separated string. `db.list_investment_events` extended with an `account_ids: Option<&[String]>` parameter. Handler resolves `profile_ids` → account set → `list_investment_events`. Test coverage added.

**Gap:** see §7.2 — CSV-string and plural-named param are inconsistent with the rest of the API; should be revisited.

### 6.4 ISO 4217 allowlist extended with `GBX` and `ZAC`

**Symptom:** `POST /api/currencies` rejected `GBX` (LSE pence) as not a valid ISO 4217 code. User couldn't add the currency the engine needed.

**Patch:** added `GBX` and `ZAC` to `VALID_ISO_CODES` in `backend/src/server/routes/currencies.rs`. Frontend `ISO_CURRENCY_NAMES` got matching entries with a separate `CURRENCY_NOTES` table for the sub-unit hint shown in the dropdown.

**Gap:** see §7.1 — this is a one-off fix; we owe a real decision on sub-unit handling.

### 6.5 Cosmetic / UX polish

- PDF footer with `Generated by fynance · {period} · {generated_at} · Page X of Y` on every page.
- S104 pool workings now formats shares (≤4 dp + thousands separator) and currency (symbol + 2 dp + native-currency tag).
- Generate PDF button has a pointer cursor.

---

## 7. Design review — open questions for the call with Timi

Reorganised from the synthesis we did after the build. Each item lists what we hit, what we tried, and the decision the call needs to make.

### 7.1 Sub-unit currencies (GBX, ZAC) — extend allowlist or convert at import?

**Today.** ISO 4217 allowlist now includes `GBX` and `ZAC` as a tactical fix (6.4). Engine treats them as first-class currencies with a user-entered `1 GBX = 0.01 GBP` rate. Importers do not convert anything at parse time.

**Two paths.**

- **A) Keep storing native-unit currencies.** Extend the allowlist further as new ones turn up. Either drop ISO strictness entirely (accept any 3-char uppercase, defer correctness to the FX rate) or move the list to config / a seeded `currencies_catalog` table so we don't ship code for every new sub-unit. Pros: data stays as the broker emitted it; one currency in, one currency out. Cons: every consumer (engine, holdings, transactions) has to know that `1 GBX = 0.01 GBP` and apply it. Multiple sub-units across brokers compound this.

- **B) Normalise at import time** — Trading 212 / Schwab / Shareworks importers detect pence and convert to GBP before write. Internal storage is then always the parent currency. Pros: engine never sees sub-units; one less FX edge case. Cons: lossy (the broker's reported price in pence is no longer in the DB), and existing data needs a migration:
  - Find every `investments`/`holdings`/`transactions` row with `currency = 'GBX'` (and 'ZAC' etc.)
  - Divide the amount/price by 100, set currency to the parent code (`GBP`, `ZAR`)
  - Update `fingerprint` rows if they include amount in the hash

**Preferred direction.** Convert at import (option B), do the migration, drop sub-unit codes from the allowlist after.

**Why this is in design review, not done:** B touches every importer and needs an actual data migration script. Worth aligning before someone (a) writes a third importer in option A's pattern or (b) bakes more engine logic that assumes sub-units exist.

### 7.2 `profile_id` contract + joint investment accounts

**Today.** `profile_ids` is a CSV string on both CGT endpoints. Other endpoints (`/api/holdings`, `/api/transactions`, `/api/budget`) use singular `profile_id` (single value). Frontend only ever sends one value.

**What to revisit.**

- Rename / replace `profile_ids` (CSV) with `profile_id` (singular) to match every other endpoint. Adding multi-profile later is cheap; living with the inconsistency is forever.
- Typical use case is one tax filer per profile (people file separately, so per-profile is the natural scope) — single-profile is the right default.
- **Open question:** can joint investment accounts exist? Joint *checking* / *savings* clearly do (we have `joint-monzo`). Can two people legally co-own a GIA brokerage? UK answer is yes (jointly-held shares). If we allow it:
  - Should the disposals attribute to both profiles' CGT?
  - 50/50 split, or a per-account ownership ratio field?
  - How does that interact with the S104 pool — one pool per profile-pair?
- Alternative: forbid `investment` / `investment_isa` / `pension` accounts from having more than one entry in `profile_ids`, validated at write time. Cleaner contract but loses real-world flexibility.

**Why this needs a call.** The answer affects schema (do we add an `ownership_ratio` table?), the engine (per-profile pools or shared), and the UI (how do we surface joint investment disposals).

### 7.3 Time-window contract: collapse to `start_date` + `end_date` on the wire

**Today.** Three params: `tax_year` (precedence-1), `start_date` + `end_date` (precedence-2), `as_at` (independent semantic). `as_at` truncates the *event ledger* before matching (pool state is point-in-time). `end_date` only filters which disposals get *emitted* (pool replays through everything). Subtle and not documented.

**Proposed contract.** Wire format is `start_date?` + `end_date?`, both optional. That's it.

- "Tax year" is a frontend concern: it computes `start_date = YYYY-04-06`, `end_date = (YYYY+1)-04-05` and sends those.
- "As at a date" is a frontend concern: it sends `end_date` only, omits `start_date`. The backend interprets absent `start_date` as "from time zero", which makes the report behave as a point-in-time snapshot — the *same* semantic as today's `as_at`.
- Drop `tax_year` and `as_at` from the wire format entirely.

**Why this is a call.** Today's `as_at` and `end_date` have *subtly different* engine behaviour (truncate-ledger vs filter-emissions). Adopting the proposal means accepting that the report use case doesn't need that distinction. If we want to keep both semantics, we should pick *one name* and document it clearly — the current overlap is the bug.

### 7.4 Tax computation in the response

**Today.** Engine returns raw `total_gains` / `total_losses` / `net_gain_loss` only. No tax number. WhatsApp thread with Timi (2026-06-07 evening) lands on adding tax — the user explicitly needs the "gan gan", not just the gains.

**Proposed shape** (refined from the WhatsApp discussion):

```jsonc
// request body, all optional
{
  "tax_config": {
    "rate_bands": [
      { "from": "2024-04-06", "to": "2024-10-29", "rate": "0.20" },
      { "from": "2024-10-30", "to": "2025-04-05", "rate": "0.24" }
    ],
    "annual_exempt_amount": "3000",
    "brought_forward_losses": "1454",
    "allowable_income_remaining": "0"   // headroom in the basic-rate band, drives lower-rate gain bucket
  }
}
```

```jsonc
// response.tax block — only present when tax_config supplied
{
  "tax": {
    "gains_by_band": [
      { "from": "2024-04-06", "to": "2024-10-29", "gain": "23601", "rate": "0.20", "tax": "4720.20" },
      { "from": "2024-10-30", "to": "2025-04-05", "gain": "11101", "rate": "0.24", "tax": "1595.28" }
    ],
    "aea_used": "3000",
    "losses_used": "1454",
    "taxable_gain": "30248",
    "tax_due": "6315.48"
  }
}
```

**Design decisions to make.**

- **Hardcode or input?** The rate-band schedule and AEA values are UK-specific and change every tax year. Three options:
  - (a) Hardcode a table on the backend, frontend never sends them.
  - (b) Frontend always sends them, with sensible UK defaults baked into the UI.
  - (c) Backend has defaults, frontend can override.
  Per the chat, hardcoding feels right *because* this tax logic is UK-only — generic doesn't help. So lean toward (a) or (c). Either way add a config table so future changes are config edits, not code.
- **Carrying over losses.** Two flavours: user types it in each year (simple, the form just has a field), or the backend stores the previous year's unused losses keyed by `(profile_id, tax_year)` and auto-fills. Storage is cleaner but means yet another schema addition. Open for the call.
- **Allowable income remaining (the basic-rate headroom).** Required for the 10% vs 20% band split. Outside this app's data scope today — we don't know the user's PAYE income. Either prompt for it as a one-off input per tax year, or stay assuming the higher rate.
- **AEA toggle.** Some users will want to model "what if I don't claim the AEA this year" — keep AEA as an optional input rather than auto-applied.
- **Mid-year rate split (2024/25 quirk)** falls out for free as a normal case of the `rate_bands` array. The engine buckets `realized_events` by `disposal_date` against the band boundaries.

**Why the call.** The shape is mostly decided; we need to agree on hardcoding vs configuring, the carryover storage model, and whether allowable-income is in scope for V1.x or pushed.

### 7.5 Defaults + override + prompting system

Generalisation of what 7.4 needs locally. Three levels:

1. **Reasonable defaults** baked into the backend / frontend for everything that has a "right" UK answer (AEA per tax year, rate bands, etc.).
2. **User override** when defaults are wrong (rate bands changed mid-year, AEA not claimed, etc.).
3. **Prompt when no default exists** (UTR, allowable income, brought-forward losses for the first year of use).
4. **Persist user inputs** so they're not re-entered next year. Probably a new `tax_inputs` table keyed by `(profile_id, tax_year)`.

Worth designing once and reusing for tax-config, reliefs, UTR, etc.

### 7.6 `S104PoolState` should carry `original_currency`

Pool values are in the symbol's native currency. The struct has no currency field. PDF + UI derive currency from `realized_events`, but a symbol that's only in the pool (no disposals in the window) falls through to `base_currency`, which is wrong. Cheap addition — add `original_currency: String` to `S104PoolState` and the engine fills it in from any pool event.

### 7.7 Disposal grouping — per-bucket vs per-rule-per-date

**Today.** Engine emits one `realized_events` row per matched bucket. 28 PLTR disposals last tax year. The user's filed SA108 has 3 PLTR "Capital Gains Disposal Summary" pages — grouped by (date, rule_applied).

**Proposal.** Response carries both:

- `realized_events: CgtRealizedEvent[]` — granular (today's shape).
- `disposal_groups: CgtDisposalGroup[]` — rolled up by `(symbol, disposal_date, rule_applied)`, sums of qty/proceeds/cost/gain, matches concatenated. Maps cleanly onto the accountant's working-page format.

The frontend chooses which to render in the schedule table and which to use for the PDF "Disposal Summary" pages.

### 7.8 Warnings channel + confidence signal

**Today.** Engine either succeeds silently or returns a 400. No middle ground.

**Proposal.** Add to the response:

```jsonc
{
  "warnings": [
    { "code": "fx_static_rate", "severity": "warn",
      "message": "FX rates are point-in-time (today's). For HMRC-grade reporting, per-date rates are required." },
    { "code": "unmatched_disposals", "severity": "warn",
      "message": "5 disposals had no matching acquisition. Their cost basis is recorded as 0.",
      "count": 5 },
    { "code": "missing_pool_currency", "severity": "info",
      "message": "Pool for FWRG has no disposals; currency inferred as base."}
  ],
  "confidence": "low"    // optional summary score across all warnings
}
```

A frontend "Health check" panel on the report can show what fired. The optional `confidence` ("high" / "medium" / "low") gives the user a one-shot sense of "should I trust these numbers".

Closely linked to 7.4 — the tax computation should include an `aea_remaining`, `losses_remaining` etc. so the user can see how the buffers are being eaten.

### 7.9 Generic error envelope, fewer special-case error codes

**Today.** Errors come back as `{ error, code }` from `AppError`. Frontend's `ApiError` class parses that into a typed exception, and each page has bespoke handling per code (e.g. CGT page has special CTA for `missing_currencies`).

**Proposal.** A single envelope shape that any handler can return, the frontend renders generically without per-code matching:

```jsonc
{
  "error": {
    "title": "Configure currencies before generating",
    "description": "Some investment events use currencies not yet configured: GBX. Add them under Settings → Currencies before generating this report.",
    "code": "missing_currencies",
    "action": {                              // optional
      "label": "Go to Settings → Currencies",
      "kind": "navigate",
      "target": "/settings/general"
    }
  }
}
```

- Backend has a `UserFacingError` helper that constructs this. Anything that can fail for a reason the user can act on returns this shape with a 4xx.
- Backend bugs (panics, DB lock, etc.) still return a generic `{ error: { title: "Something went wrong", code: "internal" } }` with 500 and no leaked details.
- Frontend has *one* error renderer that reads `title` / `description` / `action` and renders a card with a CTA button if `action` is present. No per-code switch statement.

Audit pass: walk every handler in `backend/src/server/routes/`, replace any "this throws a 500 because we panicked / unwrapped" with a `UserFacingError` (or accept it as a true internal). The fx panic was the canonical example; there are probably more.

### 7.10 Historical FX rates (the big rock)

Engine takes one rate per currency and uses it for every event regardless of date. PLTR (USD) figures last tax year differ from the filing by ~£80k because of this alone.

`convert_as_of(amount, currency, date)` already exists in `fx.rs` but currently delegates to `convert`. Real impl needs a date-keyed `exchange_rates` cache (shared with multi-currency plan §V4):

- Schema: `exchange_rates(base, quote, date, rate, source)` with `(base, quote, date)` PK.
- Provider: frankfurter.app (free, ECB) for historical lookups, cached forever.
- API: `convert_as_of` looks up the rate for the disposal/acquisition date, falls back to closest prior date if missing, errors if more than ~5 days off.
- Backfill on demand: when a CGT report is generated, the engine collects every `(currency, date)` pair it needs and fetches/caches them in one pass.

**Open for the call:** do we backfill lazily (on report request) or proactively (background job on currency change)? Lazy is simpler; proactive is faster on the user's first report.

---

## 8. Items already covered above (cross-references)

For the design review, points the synthesis raised that are already tracked elsewhere in this doc so they don't get double-counted:

- Historical FX rates: §4 → expanded in §7.10.
- Annual Exempt Amount, brought-forward losses, BADR/IR: §4 → flows into §7.4 (tax computation) and §7.5 (defaults+overrides system).
- Mid-year rate split (2024/25): §4 → handled as a special case of §7.4's `rate_bands`.
- Reverse splits: §4 — unchanged, still a follow-up.
- Unmatched-disposal UX: §4 — feeds into §7.8 (warnings channel).
- UTR capture: §4 — already added in this doc, surfaces in §7.5 as part of the prompting system.
- CSV / SA108 form-field export: §4 — unchanged, post-V1.
