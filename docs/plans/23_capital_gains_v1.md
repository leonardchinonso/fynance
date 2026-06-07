# Plan 23: Capital Gains Tax — V1

**Date:** 2026-06-07
**Status:** Backend engine shipped; V1 finishing work in progress
**Target version:** V1
**Supersedes:** Implementation portions of [`21_capital_gains_tax.md`](archive/21_capital_gains_tax.md). Plan 21 (now archived) remains the design rationale and the HMRC background reference.

---

## 1. Scope

V1 ships the HMRC-compliant CGT engine and read endpoints as a backend feature, with documentation and bindings sufficient for a frontend consumer to start building against it.

The shipped engine and tests are in PR #59 (commit `c6ec43f`). This plan covers what's done, what still needs to land before V1 is closeable, and what is explicitly deferred to V1.1+ or V2.

---

## 2. Shipped in PR #59

Engine, routes, and tests:

- [`backend/src/server/routes/capital_gains.rs`](../../backend/src/server/routes/capital_gains.rs) — 770-line engine implementing same-day FIFO, 30-day Bed & Breakfast, S104 pool replay, and unmatched-disposal remainder, with `tax_year` / `start_date`–`end_date` / `as_at` filtering and ISA/Pension exclusion via `accounts.account_type`.
- [`backend/tests/capital_gains.rs`](../../backend/tests/capital_gains.rs) — 549-line integration test covering each matching rule independently, ISA exclusion, and the tax-year query.
- Endpoints registered in [`server/mod.rs`](../../backend/src/server/mod.rs):
  - `GET /api/investments/pools?as_at=` — S104 pool snapshot
  - `GET /api/investments/capital-gains` — full CGT report

ts-rs bindings: `CapitalGainsResponse`, `CgtSummary`, `SymbolSummary`, `CgtRealizedEvent`, `CgtMatchDetail`, `S104PoolState` are emitted to `frontend/src/bindings/`.

---

## 3. V1 Finishing Work

These items close out V1 of the backend feature so the frontend can begin V1.1 against a documented API.

- [x] Regenerate ts-rs bindings — `CapitalGainsResponse.ts` and `SymbolSummary.ts` were stale in PR #59. Re-running `cargo test` in `backend/` refreshes them.
- [x] Add a design doc at [`docs/design/08_cgt_engine.md`](../design/08_cgt_engine.md) describing inputs, the matching algorithm, the API, what's stored vs computed, and known limitations.
- [x] Update [`CLAUDE.md`](../../CLAUDE.md) REST API surface section with the two new endpoints.
- [x] Update the hand-crafted OpenAPI spec at [`backend/src/server/routes/docs.rs`](../../backend/src/server/routes/docs.rs) with paths and schemas for `/api/investments/pools` and `/api/investments/capital-gains` so external agents can discover them.

That's the V1 backend close-out. No code changes to the engine itself.

---

## 4. Out of Scope for V1

Deferred to V1.1+ or V2. Tracked here for traceability; do not bundle into V1.

- **Frontend UI.** The Portfolio "Investments" view, the S104 pool viewer, the CGT summary on the Reports tab, and the SA108-style document export are all V1.1+. The engine is intentionally backend-only until the API stabilises.
- **Historical FX rates.** Plan 21 §2 calls for per-leg date-keyed FX conversion (acquisition rate at acquisition date, disposal rate at disposal date). The shipped engine uses the current preferred-currency rate from `currencies` for every event. A date-keyed `exchange_rates` cache is a V2 concern shared with multi-currency work (see [`22_multi_currency.md`](22_multi_currency.md) §V4).
- **Annual exempt amount and rate-band logic.** The engine returns raw `total_gains` / `total_losses` / `net_gain_loss`. Applying the AEA and computing tax due is left to the consumer.
- **Reverse splits and consolidations.** The `Split` branch scales `pool_shares` by `quantity`; forward splits work correctly but reverse splits (ratio < 1 in `quantity`) and share consolidations have no test coverage.
- **Unmatched-disposal UX.** Disposals that exhaust all three matching rules are emitted with `cost_basis = 0` and `rule_applied = "Unmatched"`. The frontend will need to surface these as a data-quality warning, not a real gain.

---

## 5. Acceptance for V1

V1 is done when:

1. The two endpoints respond under all documented query combinations (covered by `tests/capital_gains.rs`).
2. ts-rs bindings include `symbol_summaries` and the `SymbolSummary` type.
3. `docs/design/08_cgt_engine.md` exists and matches the implementation.
4. `CLAUDE.md` lists both endpoints under the REST API surface.
5. `GET /api/docs` returns OpenAPI entries for both endpoints with schemas for the response types.
6. CI green on the branch (it already is).

When all six are checked, PR #59 can merge as the V1 backend deliverable.
