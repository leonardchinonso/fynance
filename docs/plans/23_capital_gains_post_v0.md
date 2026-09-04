# Plan 23: Capital Gains Tax — Post-V0

**Date:** 2026-06-09 (updated 2026-08-15)
**Status:** Backend engine + V0 finishing work shipped; report UI in flight; **§7 design questions resolved 2026-08-15 — see §0**
**Target version:** rolling, post-V0
**Supersedes:** Implementation portions of [`21_capital_gains_tax.md`](archive/21_capital_gains_tax.md). Plan 21 (now archived) remains the design rationale and the HMRC background reference.

> **⚠️ Reference values in this document are illustrative placeholders — keep them that way.**
> UTRs, filed tax amounts and proceeds figures below are invented stand-ins (UTRs use the
> `1234567890` placeholder form, matching the fixture convention in
> `frontend/src/data/mock_profiles.ts`). **This repository is public — never substitute a real
> HMRC reference or a real filed figure for realism.** The worked examples are there to show the
> *shape* of the arithmetic; they do not need genuine numbers to do that, and the arithmetic
> below is internally consistent on the placeholder values.

---

## 0. Decisions — 2026-08-15

The §7 design questions were worked through and answered. §7 is retained below as the rationale and
the detail; **this section is what was decided.** Where the two disagree, this section wins.

### 0.1 What the filed 2024-25 return actually shows

Several assumptions in this doc were checked against the real filed return (SA100/SA108 for the year
ended 5 Apr 2025) rather than inferred. Corrections:

- **CGT due was £6,250.00, not £6,251.32.** The £6,251.32 figure quoted in §7.4 is the *theoretical*
  tax. HMRC's system does not compute it that way: it charges everything at the **old 20% rate**
  (£30,000 × 20% = £6,000.00), then adds a **CGT51 adjustment rounded DOWN to whole pounds**
  (£250.48 → £250.00). £6,000.00 + £250.00 = £6,250.00.
  **❌ We are not reproducing this** — see 0.2.
- **The highest-rate-band-first ordering is confirmed correct.** The Intermediary Summary shows both
  the £1,454 current-year loss and the £3,000 AEA deducted from the **post-30-Oct (24%)** band,
  leaving £6,647, with the pre-30-Oct £23,601 untouched. This was previously an assumption.
- **Brought-forward losses were £0** for 2024-25, and income exceeded the higher-rate threshold with
  no basic-rate band left — so every gain sat in the upper band. `allowable_income_remaining: 0` is
  the correct default for this user.
- **Filed proceeds £232,000 ÷ sells-only $295,000 = an implied rate of 0.7864**, against the 0.74
  configured in `currencies`. That ~6% gap is the entire reason the report does not tie. Confirms
  §7.10 as the sole remaining cause on the gains side.

### 0.2 Decisions

| # | Decision |
|---|---|
| **7.1** | **Convert sub-units at import.** A static subunit→parent mapping (`GBX → GBP ÷ 100`, `ZAC → ZAR ÷ 100`), not FX rates. The current rates approach only works while the preferred currency is GBP; switch it to USD and GBX needs 0.0074 maintained separately from GBP's rate, and they drift. Historical FX makes it worse — a GBX series is just the GBP series ÷ 100. **The list is short and closed in practice: `GBX` (British pence), `USX` (US cents), `ZAC` (South African cents), `ILA` (Israeli agorot).** These are *unofficial market conventions*, not ISO 4217 — which is exactly why they are hardcoded: there is no authoritative list to fetch, and other portfolio tools hardcode the same four. Own PR: mapping + importers + migration + drop from the allowlist. |
| **7.2** | **Keep the API flexible; make CGT fail loudly.** `profile_ids` stays as-is and joint accounts remain representable — the data model should not forbid something lawful. But the **CGT endpoints refuse** when any in-scope account has more than one `profile_id`: `400` with a message along the lines of *"Cannot calculate capital gains for an investment account with multiple owners."* Rationale: the S104 pool has no concept of shared ownership, so today a joint GIA returns 100% of the gain to *each* profile, double-counting across two returns. Failing loudly beats silently computing the wrong number, and it scopes the breakage to the one computation that is genuinely ambiguous — everything else keeps working. Today: one joint account (Monzo Joint, `checking`), zero joint investment accounts. Revisit if ownership ratios are ever modelled. |
| **7.3** | **Collapse the wire format to `start_date` + `end_date`; drop `tax_year` and `as_at`.** A breaking change, accepted deliberately in favour of the less error-prone contract. The two params genuinely differ — `as_at` truncates the event ledger so the 30-day rule cannot reach forward, while `end_date` only filters emissions so it *can*, giving the same disposal a different cost basis depending on which is used. That overlap is the bug. "Tax year" and "as at" become frontend arithmetic, which is where they already live: `cgt_filter_params.ts` sends only `start_date`/`end_date` today, so nothing in the app changes. Absent `start_date` means "from time zero", reproducing today's `as_at` semantics for the report use case. |
| **7.4 / 7.5** | **Server-side, two new tables.** `tax_config` (rate bands, AEA per tax year, split dates — the law, seeded with UK values, overridable) and `tax_inputs` (brought-forward losses, allowable income remaining, AEA-claimed — the user's situation, keyed `(profile_id, tax_year)`). Two tables so a Budget change can reseed statutory values without touching personal inputs. **Must implement the CGT51 two-step** per §0.1. |
| **7.6** | **Done 2026-08-15.** `original_currency` added to `S104PoolState` as a mandatory field, and the base-currency fallback bug it existed to fix was fixed in `cgt_pool_workings.tsx`. No migration — it is a computed response DTO, never persisted. |
| **7.7** | **Group by `(symbol, disposal_date)`**, not the `(symbol, disposal_date, rule_applied)` in §7.7 below and not by rate band. The engine emits one row per *matched bucket*, so a single sale that matches same-day + 30-day + S104 becomes three rows — an artifact of the matching rules, not three sales. Rolling up per actual sale gives the honest answer to SA108 box 23. Rate-band bucketing belongs in the tax computation (7.4), not in presentation. Keep granular `realized_events` alongside. |
| **7.8** | ❌ **Won't Do — dropped entirely.** Every candidate warning turned out to be an error. `fx_static_rate` is fixed by 7.10; `missing_pool_currency` was fixed by 7.6; `unmatched_disposals` blocks; a joint account in scope is a 400 (7.2); a symbol in two currencies is a 400; a consolidation exceeding its pool is a 400. Nothing was left to warn about. **A warnings array is a way of deferring the fail-or-pass decision — once every case is decided, it evaporates.** The one thing worth keeping from it is *provenance, not warnings*: record each rate's `source` so the artifact can show which rate was used and where it came from. That is an audit trail, not a caveat. |
| **7.9** | **Moved out of this plan.** A codebase-wide error-envelope refactor with nothing to do with capital gains; it landed here only because the CGT UI tripped over it. Tracked in `20_post_v0_plans.md` alongside the `StorageError` work it is coupled to. |
| **7.10** | **P0. User-owned rates — see §0.3.** |
| **7.11** | **Agreed, but sequenced last.** Blocked on the `db.rs` split (plan 20 §V2) and on 7.10 + 7.4 landing first, or the diffs become unreviewable against a ~900-line file move. |
| **BADR / IR** | ❌ **Won't Do** — see §4. |
| **CGT51 two-step** | ❌ **Won't Do.** HMRC's calculator predates the 30 Oct 2024 rate change, so a mid-year return is computed at the old 20% rate and corrected via an adjustment box rounded down to whole pounds. Reproducing it would move our 2024-25 figure by **88p**. Dropped for three reasons: it is a **one-year artifact** (2025-26 onward sits wholly after the change, single rate, no adjustment); the complication is not worth 88p; and the "it validates the engine against a known-good filing" argument **does not survive our own sell-to-cover decision** — we deliberately include disposals the filed return omits, so our totals cannot tie to it regardless, and a tie was never available to be used as a test. ⚠️ **This drops only the presentational two-step, NOT the rate split**: bucketing gains pre/post 30 Oct and charging 20% vs 24% is still required and stays in scope. |
| **Unmatched disposals** | **Block, do not warn.** `cost_basis = 0` counts 100% of proceeds as gain and **overstates tax**. A genuine short sale is effectively nonexistent in a retail portfolio; in practice it always means missing acquisition data. |
| **Mixed currency per symbol** | **Error, at both write and read time.** One symbol whose events carry two currencies makes the S104 pool a sum of pence and pounds — a meaningless total. Realistic cause: the same LSE holding reported as `GBX` by one broker and `GBP` by another (7.1 removes that cause, but ticker collisions and data-entry errors remain). It is not blocked at DB level today because there is no symbols table — `symbol` is a TEXT column per event, so there is nowhere to hang the constraint. Follow the existing `validate_currency` / `check_required_currencies` pattern: reject at write time, and precheck at report time for rows predating the guard. |
| **Consolidation exceeding its pool** | **Error — `consolidation_exceeds_pool`. Implemented 2026-08-15.** Removing more shares than the pool holds is impossible data (mistyped quantity, missing acquisition, wrong symbol). Clamping at zero — the first cut of this — leaves a pool with no shares but non-zero cost, so average cost becomes zero and every later disposal reports 100% of proceeds as gain: tax overstated, output looks perfectly ordinary. `run_cgt_engine` now returns `Result<_, AppError>` to carry refusals like this; the unmatched-disposal block reuses that signature. |
| **Currency referential integrity** | **Make the unconfigured-currency state unreachable, from both ends.** Writes are already guarded — `validate_currency` runs on accounts, holdings, investments (currency *and* fee currency) and import. Deletes are **not fully guarded**: `Db::delete_currency` refuses when a code is used by `holdings`, `accounts` or `transactions`, but **does not check `investments`** — so a currency in use by investment events can still be deleted, which is precisely how the "configured currency vanished" state is reached. Add `investments` (both `currency` and `fee_currency`) to that guard. Once both ends hold, `missing_currencies` becomes genuinely unreachable rather than merely rare. |
| **`missing_currencies`** | **Absorbed by the pre-flight step (§0.4).** It currently conflates two things: a currency with no row in `currencies` at all, and a rate that cannot be applied. Write-time validation already prevents the first for new data, so it degrades to a defensive internal error rather than a user-facing CTA; the second becomes the pre-flight "rates we need" list. The bespoke `missing_currencies` CTA card therefore leaves the frontend — worth noting, since it was the motivating example for the (now relocated) §7.9 error envelope. |
| **Sell-to-cover** | **Included as disposals — a deliberate divergence from common practice.** Documented in `docs/design/08_cgt_engine.md` § Deliberate Divergences and in-code at the disposal branch. Reports will not tie to a computation prepared the other way; that is intentional. |

### 0.3 Historical FX (§7.10) — the rates are the user's, not a provider's

The §7.10 design below assumes the engine fetches rates from frankfurter.app and consumes them.
**Inverted:** rates are user-owned data; auto-fetch is an optional frontend convenience that fills in
a field the user still approves.

This is not just a preference. HMRC does not mandate a rate source — *"The law does not provide for a
particular exchange rate basis... taxpayers can use alternative rates from reputable sources. The
basis chosen should be used consistently."* A user-entered rate is therefore fully legitimate, and
silently fetching ECB dailies would be **less** correct here, because it would not reproduce the
rates a previously-filed return was computed with.

The per-leg requirement still binds absolutely: HMRC explicitly rejects computing a gain in foreign
currency and converting the result. Acquisition converts at the acquisition-date rate, disposal at
the disposal-date rate. **The engine already does this** — `run_cgt_engine` calls `convert_as_of` at
every conversion site with the correct per-leg date, including passing the *acquisition* date for
30-day matches. Only `convert_as_of`'s body is missing; no call-site changes are needed.

**Scale (measured, not estimated).** 885 investment rows total, of which 256 are non-GBP in
CGT-eligible accounts → **68 distinct `(currency, date)` pairs for all history**. A 2024-25 report
needs **49** of them: 17 disposal dates plus 32 cumulative acquisition dates. Note the second number:
the S104 pool is built from *every* acquisition ever, so prompting only for dates inside the tax year
would silently use wrong cost bases.

**No gap policy is needed.** Nothing is auto-resolved, so weekends and bank holidays are not a
special case — a missing rate is a prompt, not a fallback.

### 0.4 The pre-flight review step

Rather than three separate blockers (a 400 for FX, a settings field for UTR, an unanswered question
for losses), generation is preceded by **one review screen** that surfaces everything needing
confirmation:

- **Missing currency conversions** for the `(currency, date)` pairs this report needs — filled in
  here, stored, never asked again. Optional auto-fill button if a rate provider is configured; the
  stored value is still whatever the user accepted.
- **The UTR being used**, so it is confirmed rather than silently pulled from settings.
- **Carried-forward losses** — backend-derived prefill, editable.
- **The AEA** for the year, editable.

This replaces "missing rate → 400" entirely: a missing rate is neither an error nor a warning, it is
a pre-flight item resolved before generation is offered.

**Brought-forward losses are derived AND overridable.** Derivation is correct whenever fynance holds
the history; the override covers every year filed elsewhere, which is currently all of them. Two
requirements on the prefill: it must be **visibly labelled as derived**, showing which years it came
from, and it must not look authoritative. UK losses must be *claimed* within four years of the end of
the tax year in which they arose, and only the excess after same-year gains carries forward — so a
naive "sum of past losses" prefill can overstate.

**The generated artifact shows the rate used per disposal**, so the report is auditable against
whatever basis was chosen.

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

### BADR / Investors' Relief — ❌ Won't Do (for now)

**Decision 2026-08-15: not building this.** Neither relief is reachable from the portfolio this app
actually tracks, and the filed 2024-25 return has every BADR/IR box at zero (SA108 boxes 17.2–17.4,
and the "Gains not qualifying for Business Asset Disposal Relief or Investors' Relief" heading
carries the whole computation).

- **BADR** (Business Asset Disposal Relief, formerly Entrepreneurs' Relief) applies to disposing of
  *your own business* — a sole trade, a partnership interest, or shares in a personal trading
  company where you have been an employee or officer for at least 2 years and hold at least 5% of
  ordinary share capital and voting rights. Rate 14% for 2025-26, rising to 18% from 6 Apr 2026.
  £1m lifetime limit.
- **Investors' Relief** is the passive-investor counterpart: newly-issued unlisted trading company
  shares subscribed for and held at least 3 years, where you are *not* an employee or officer. Rates
  now aligned with BADR (14% → 18%), and the lifetime limit was cut from £10m to £1m on
  30 Oct 2024.

Neither can apply to listed shares and securities (PLTR, ETFs, funds), which is the entire holding
base here. **Revisit if founder equity, an unlisted subscription, or a business disposal ever
enters the picture** — at which point the original sketch stands: a per-disposal "relief claimed"
flag, the reduced rate applied to qualifying gains, and lifetime-limit tracking across tax years.

_Original plan, retained for whenever this is picked up:_ per-disposal flag and reduced rate. UI: a
checkbox on each row of the disposal schedule to mark "BADR claimed". Total relief-qualifying gains
roll into the tax computation at the reduced rate.

### Mid-year rate-split (2024/25 specifically)

UK CGT main rates increased on 30 Oct 2024: 10%/20% → 18%/24%. The SA108 has a "Capital Gains Adjustment Summary" page that splits disposals into pre/post-30-Oct buckets and applies different rates to each. Year-specific quirk but a real one — needed for any actual filing covering 2024/25.

Implementation: bucket `realized_events` by `disposal_date` against 2024-10-30, apply per-bucket rates from a config table. The config table also makes future rate changes a config edit rather than a code change.

### UTR capture and report attribution

The user's HMRC Unique Taxpayer Reference is the primary identity field on every SA108 page (e.g. `UTR: 1234567890`, the placeholder form — see the note at the top of this document). Capture it once at the user/profile level, then include it on the generated PDF's running footer alongside the existing `Generated by fynance · {tax year} · {generated_at} · Page X of Y`.

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

~~**Gap that remains:** investment-event ingest (`POST /api/investments/import`) still has no `validate_currency` call, so the data can keep arriving in unknown currencies. The precheck catches it at read time but the right fix is at write time.~~

**✅ Closed (verified 2026-08-15).** Investment-event ingest now validates at write time: [`investments.rs`](../../backend/src/server/routes/investments.rs) checks both `currency` and `fee_currency` via `validate_currency` before the write. `FxRateMap::convert` documents the same invariant — a currency missing from the FX table can now only come from a row written before that validation existed. The read-time precheck in 6.2 stays as defence-in-depth, but it is no longer the only guard, and this is **not** blocked on the §7.1 direction.

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

**Status (2026-06-26): an interim, client-side version of this shipped.** The CGT PDF report now applies the Annual Exempt Amount, splits 2024-25 gains at the 30 Oct 2024 rate change, and estimates the tax due, with a higher-vs-basic rate toggle on the report filter bar (stored frontend-only on `StoredCgtReport.higherRate`, not on the backend-contract `CgtFilters`). The AEA table and the rates are **hardcoded** in [`cgt_pdf_document.tsx`](../../frontend/src/pages/reports/cgt/cgt_pdf_document.tsx) and flagged temporary in-code. This section remains the target end-state: move the computation server-side, accept a `tax_config` input on the endpoint, and persist user-definable rates/allowances so a Budget change is a config edit, not a code change. The toggle's `higherRate` becomes the `tax_config` rate-band selector (or the `allowable_income_remaining` headroom) when this lands.

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

**Implementation sketch (changes needed).** Concrete work when this lands, so it isn't re-derived. This replaces the interim frontend version (status note above):

- **Backend ([`capital_gains.rs`](../../backend/src/server/routes/capital_gains.rs)):**
  - New `TaxConfig` input (from the request) and `TaxResult` output (`gains_by_band`, `aea_used`, `losses_used`, `taxable_gain`, `tax_due`), both `#[derive(TS)]` exported to `frontend/src/bindings/`. Add `tax: Option<TaxResult>` to `CapitalGainsResponse`, populated only when a `tax_config` (or a stored/default config) is present.
  - Compute after `run_cgt_engine`: bucket `realized_events` by `disposal_date` against `rate_bands` (the 2024-25 30 Oct split is just two bands), sum gains per band, then **deduct brought-forward + current-year losses and the AEA from the highest-rate band first.** That ordering minimises the charge and reproduces the accountant's working (verified against the filed figure for 2024-25 — see §0.1; the amounts there are illustrative placeholders). `allowable_income_remaining` moves that much gain into the lower-rate band; absent it, assume all higher-rate.
  - Money stays `rust_decimal::Decimal` end-to-end, replacing the interim frontend float math.
  - Config source per §7.5: a seeded `tax_config` table or `config/uk_tax.yaml` so the rate/AEA/split-date tables are data, not code. Lazy external fetch deferred.
- **Bindings:** regenerate ts-rs after the struct changes (export runs under `cargo test`).
- **Frontend ([`cgt_pdf_document.tsx`](../../frontend/src/pages/reports/cgt/cgt_pdf_document.tsx) + [`cgt_filter_bar.tsx`](../../frontend/src/pages/reports/cgt/cgt_filter_bar.tsx)):** delete `AEA_BY_TAX_YEAR`, `computeTaxEstimate`, and the hardcoded rate literals; render `response.tax` directly. Keep the rate-band toggle, but have it set the request's band selector / `allowable_income_remaining` rather than a client-side flag, and send the selection via `cgtFiltersToParams`.
- **Cleanup:** the TEMPORARY-flagged tables in `cgt_pdf_document.tsx` are removed as part of this; they exist only until the backend owns the computation.

### 7.5 Defaults + override + prompting system

Generalisation of what 7.4 needs locally. Three levels:

1. **Reasonable defaults** baked into the backend / frontend for everything that has a "right" UK answer (AEA per tax year, rate bands, etc.).
2. **User override** when defaults are wrong (rate bands changed mid-year, AEA not claimed, etc.).
3. **Prompt when no default exists** (UTR, allowable income, brought-forward losses for the first year of use).
4. **Persist user inputs** so they're not re-entered next year. Probably a new `tax_inputs` table keyed by `(profile_id, tax_year)`.

**Sourcing the defaults without a code change.** The statutory tables (AEA, rate bands, the mid-year split date) should be *data*, not code, so an HMRC/Budget change doesn't need a frontend/Rust edit and redeploy. Options, roughly in order of preference: a seeded `tax_config` table the user/admin can edit in Settings; a checked-in config file (e.g. `config/uk_tax.yaml`) loaded at startup; or an on-demand fetch from an authoritative source (the "curl for it" idea — the same lazy-fetch-and-cache pattern §7.10 uses for FX, pointed at a tax-rate feed). The hardcoded tables now in [`cgt_pdf_document.tsx`](../../frontend/src/pages/reports/cgt/cgt_pdf_document.tsx) are the temporary opposite of this and should be the first thing replaced when §7.4 lands.

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

### 7.9 Generic error envelope — ➡️ MOVED to `20_post_v0_plans.md`

**Moved 2026-08-15.** This is a codebase-wide error-handling refactor with nothing specific to
capital gains; it was filed here only because the CGT report UI was the first thing to trip over it.
It now lives in [`20_post_v0_plans.md`](20_post_v0_plans.md) under **Backend Hardening**, next to the
typed `StorageError` work it must be implemented alongside.

<details>
<summary>Original §7.9 text, retained for history</summary>

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

The storage-side half of this (a typed `StorageError` enum replacing the message-substring matching that routes do today, plus parameterizing the one string-built query) is tracked in `20_post_v0_plans.md` under "Backend Hardening"; implement the two together so handlers map `StorageError` variants straight into this envelope.

</details>

### 7.10 Historical FX rates (the big rock)

Engine takes one rate per currency and uses it for every event regardless of date. PLTR (USD) figures last tax year differ from the filing by ~£80k because of this alone.

**Update (2026-06-26):** with the Shareworks ledger rebuilt (gross vests), static FX is now the *only* remaining reason the report doesn't tie to the filed return on the **gains** side (the losses gap is the deliberate sell-to-cover treatment). For 2024-25, fynance reports PLTR gains-before-losses of £31,877.80 vs the filed £34,702 (~£2.8k); HMRC converts each leg at its own date's rate while we convert every leg at one flat rate, and the configured 0.74 also runs below the ~0.78 GBP/USD average for the period. Confirmed cause, tracked here as the fix. Until it lands, USD positions will not tie to the penny and the report's FX footnote says so.

`convert_as_of(amount, currency, date)` already exists in `fx.rs` but currently delegates to `convert`. Real impl needs a date-keyed `exchange_rates` cache (shared with multi-currency plan §V4):

- Schema: `exchange_rates(base, quote, date, rate, source)` with `(base, quote, date)` PK.
- Provider: frankfurter.app (free, ECB) for historical lookups, cached forever.
- API: `convert_as_of` looks up the rate for the disposal/acquisition date, falls back to closest prior date if missing, errors if more than ~5 days off.
- Backfill on demand: when a CGT report is generated, the engine collects every `(currency, date)` pair it needs and fetches/caches them in one pass.

**Open for the call:** do we backfill lazily (on report request) or proactively (background job on currency change)? Lazy is simpler; proactive is faster on the user's first report.

### 7.11 Move the engine out of the routes layer

`run_cgt_engine` and its supporting types (~900 lines of matching-rule business logic) live inside the HTTP route file, [`backend/src/server/routes/capital_gains.rs`](../../backend/src/server/routes/capital_gains.rs). Move them to a dedicated `backend/src/cgt/` module, leaving the route as a thin adapter that parses query params and maps the engine result into the response. Pure relocation, no behavior change; the integration tests pin the outputs.

While there, add cross-references between the engine and the second average-cost pooling implementation in `Db::get_investment_history`: they intentionally differ (history applies plain S104 averaging and skips same-day/30-day matching, which is fine for a value-over-time chart), but nothing at either site says so today, which invites someone to "fix" one to match the other.

Sequencing: best done after the `storage/db.rs` module split tracked in `20_post_v0_plans.md` §V2 Refactoring, so each diff stays a pure file move.

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
