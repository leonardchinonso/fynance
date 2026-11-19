# 31 — View state in the URL: Tax page and report screens

Plan only. No code touched. Written 2026-11-19.

## The rule

There is **no verbatim "state must live in the URL" rule** in `CLAUDE.md`, `docs/design/`, `docs/plans/` or `.claude/memory/` (searched all four plus shared hub memory). The standard is established in code, in the shared hooks every compliant page uses:

- `frontend/src/hooks/use_page_size.ts:6-12` — the clearest statement: "Page size stored solely in a URL query param — **the URL is the single source of truth (no localStorage)**, so an empty URL is always the default 25 and the value **round-trips in shared/pinned links**, exactly like the other filters. The param is omitted at the default to keep URLs clean."
- `frontend/src/hooks/use_url_filters.ts:126-128` — "Only the preset goes in the URL; start/end are derived at read time so **bookmarks and pinned views stay evergreen**."
- `frontend/src/pages/import/tax/tax_import.tsx:34-36` — "The tax-document import flow, **derived from the URL** like the statement flow: `/import/tax` is setup, `?entry=<id>` is the preview, `?done=1` the summary."
- `docs/plans/23_capital_gains_post_v0.md:155` — "Generated reports persist in localStorage and **have their own URL** (`/reports/cgt/:reportId`)."

Standard taken from these: **every piece of view state a user chooses (filter, tab, year, profile, page, sort, wizard step) is reconstructable from the URL alone; defaults are omitted from the URL; invalid values fall back to the default silently.** Step 6 of this plan codifies the rule in `CLAUDE.md` Conventions so it stops being folklore.

## Violations (audited via git log since 2026-10-01 over `frontend/src/pages`)

| # | File:line | State | Lost on refresh | Back/forward | Deep link |
|---|---|---|---|---|---|
| 1 | `frontend/src/pages/Tax.tsx:44` | `taxYear` in `useState`, defaults to current year | Yes, silently reverts to current year | No history entries for year changes | Cannot link `/reports/tax?year=2024-25` |
| 2 | `frontend/src/pages/Tax.tsx:51,104` | `pickedProfile` in `useState`; the dropdown (only shown when the URL profile is invalid, `Tax.tsx:103`) writes only local state | Yes, falls back to localStorage/first profile; picked value never reaches the URL | No | Cannot link a taxpayer |
| 3 | `frontend/src/pages/Tax.tsx:59` | `dialog` (source / record / check / expense-mapping editor) in `useState` | Editor closes | Back button does not close the editor | Cannot link "edit record X" |
| 4 | `frontend/src/pages/reports/documents/documents_page.tsx:298-307` | `orphanFilter`, `selectedAccounts`, `search`, `page`, `sort`, `sortDir` all in `useState` | All six reset to defaults | No | Cannot link a filtered document list |
| 5 | `frontend/src/pages/reports/cgt/cgt_filter_bar.tsx:33-36,45` | CGT filter form: period preset, start, end, profileId, band selection in `useState` | Yes, reverts to previous tax year + navbar profile | No | Cannot share "CGT for 2023-24" before generating (saved reports *do* have URLs; the filter view does not) |
| 6 | `frontend/src/pages/import/tax/tax_documents_table.tsx:74` | Expanded rows in `useState` inside the tax-import preview | Expanded rows collapse (wizard steps themselves are URL-driven) | No | Minor |
| 7 | `frontend/src/pages/import/tax/tax_import.tsx:48` | Setup-form profile pre-filled from `?profile=` but changes are not written back to the URL | Reverts to pre-fill | No | Minor |

**Not violations (deliberate preferences, keep as-is):** column visibility (`transaction_table.tsx:297`, `events_history.tsx:204`, `documents_page.tsx:303`), `budget_spreadsheet.tsx:69` show-empty, homepage (`use_homepage.ts`), theme, redaction, API mode. These are per-user settings, not view state; localStorage is the established home for them. `budget.tsx:123` `selectedTxnIds` is a transient bulk-selection, out of scope.

**Compliant baseline to mirror:** `useUrlFilters` (`use_url_filters.ts`) drives Budget/Portfolio/Transactions/Investments; `useUrlState` (`use_url_state.ts`) is the thin many-small-params wrapper; wizard steps in `import.tsx` / `tax_import.tsx` are URL-derived.

## Fix design

Reuse the existing hooks; no new framework.

- **Tax page (`/reports/tax`)**: drop `taxYear` and `pickedProfile` `useState`. Year: `?year=2025-26` via a small `use_tax_params.ts` hook built on `useUrlState`. Profile: use `useUrlFilters().profileId / setProfileId` exactly like every other page (writes both `?profile=` and the `fynance-profile` localStorage fallback), which also fixes the dropdown hiding itself whenever a profile param is present — show it whenever `profiles.length > 1`.
- **CGT filter view**: lift the filter bar's state into `useUrlState` params: `period` (preset slug or `custom`), `start`/`end` (only when custom), `band`. Profile already flows through `useUrlFilters`. Saved-report view keeps its `/reports/cgt/:reportId` route unchanged.
- **Documents page**: params `q`, `accounts`, `state` (`all|orphaned|linked`), `page`, `sort`, `dir`; reuse `usePageSizeParam` for page size like the other tables. Column visibility stays localStorage.
- **Tax editor dialogs** (#3, last step, separable): `?edit=source/<id>`, `?edit=record/<source>/<id>`, `?edit=check/<source>/<id>`, `?edit=mapping/<source>`. Param restore reopens the dialog; typed-but-unsaved form fields are not restored and that is accepted.

Conventions to apply, all lifted from the existing hooks:

- **Defaults omitted**: `year` omitted = current tax year; `period` omitted = previous tax year (CGT today); `state`/`sort`/`dir` omitted = today's defaults. Keeps URLs clean and evergreen.
- **Invalid values**: validate `year` against the `YYYY-YY` shape (reuse `ukTaxYearToDates` from `api/cgt_filter_params.ts`, which already throws on malformed input) and resolve against `yearOptions`; unknown values render the default and the param is dropped on the next write (same posture as `use_page_size`'s `PAGE_SIZE_OPTIONS.includes` check). Invalid profile keeps the existing `activeIsValid` fallback (`Tax.tsx:50`).
- **Push vs replace**: user-initiated filter/year/profile changes **push** (so Back walks through them; this matches `useUrlFilters.setFilter`, which pushes). The only reason `useUrlState` currently forces `replace` (`use_url_state.ts:33`) is high-frequency writers; add an optional `{ push: true }` to `useUrlState.set` and use push for the Tax/CGT/Documents controls and dialog open (Back closes a dialog), keeping replace for nothing in this plan. Dialog close removes the param.

## Steps (ordered, independently shippable)

1. `useUrlState.set` gains an optional push mode (touch no caller). Unit-testable helper `resolve_tax_params(yearRaw, profiles, …)` in `frontend/src/pages/tax/tax_params.ts` with vitest (mirror `tax_helpers.test.ts`).
2. Tax page: wire `year` param + `setProfileId`; delete the two `useState` lines; dropdown always when >1 profile. Fixes the reported refresh bug (#1, #2).
3. Documents page: URL params + `usePageSizeParam` (#4); extract a `documents_params.ts` pure serializer/resolver + vitest.
4. CGT filter view: URL-backed filters via `useUrlState` push (#5).
5. Tax dialog params (#3), plus #6/#7 only if cheap while in the file.
6. Update `frontend/scripts/smoke_preview.mjs` (CLAUDE.md requires same-PR smoke updates): on `/reports/tax`, pick year 2024-25, reload, assert the trigger still reads 2024-25; also load the deep link cold. Add a Documents filter+reload step. Add the rule line to CLAUDE.md Conventions.

## Tests (run once, at the review gate)

- `npm run test` (vitest): new `tax_params` and `documents_params` tests covering defaults, invalid values, round-trip serialize.
- `tsc --noEmit` + `npm run build` (gating CI jobs).
- `node scripts/smoke_preview.mjs` once, desktop + mobile (covers steps 2 and 6 end-to-end).
- No backend changes; no cargo runs.

## Risks

- `profile` param is shared site-wide through `useUrlFilters` + localStorage: changing profile on Tax also changes it on Budget/Portfolio. That is the existing app-wide behaviour everywhere else; Tax should join it, not fork it.
- Push-history for filters means more Back presses to leave a page; accepted (matches Budget today).
- `useUrlState` replace-forcing change touches shared code; keep it additive and defaulted to today's behaviour so existing callers (`import.tsx` etc.) are untouched.
- CGT preflight (`cgt_report_page.tsx:73`) holds filters in state mid-error; URL-backed filters make retry-on-refresh simpler, not harder.
