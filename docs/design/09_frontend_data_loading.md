# Frontend Data Loading: demand-driven loads, request-keyed cache, LCA placement

Status: implemented (issue #52).

This document is the rule + reference for how the React frontend loads server data:
when a fetch fires, how results are cached and deduplicated, how writes keep the
cache honest, and where in the component tree a load should live.

## Problem

The old `useRemoteData` hook fetched on mount with no cache and no `enabled`
gate. Two consequences:

- **Eager loads.** A page fetched every dataset it *might* render, not the one it
  *was* rendering. `portfolio.tsx` hit the Overview, Accounts, and History
  endpoints on mount regardless of the active tab. This amplified the ~10s
  Portfolio load before the net-worth perf fix (PR #51): landing on Overview
  still paid the slow History query.
- **No persistence across navigation.** Navigating away from a view and back
  refetched everything, even when nothing had changed.

## Decision: in-house keyed cache (not TanStack Query)

We built a small in-memory, request-keyed cache rather than adopting
`@tanstack/react-query`. Rationale:

- **Scope is small and well understood.** The features we need — request-keyed
  cache, per-query TTL, in-flight dedup, explicit invalidation — are ~200 lines
  (`lib/query_cache.ts` + `lib/query_key.ts` + `hooks/use_query.ts`). TanStack
  brings far more surface than we would use.
- **Preserves the existing contract.** Every component consumes the `RemoteData`
  union via `visitRemoteData`. The in-house `useQuery` returns the *same*
  `[RemoteData<T>, refresh]` shape, so migrating the ~16 data hooks was
  mechanical and **zero** consumers changed. A TanStack migration would mean
  rewriting all the hooks *and* moving every consumer off `RemoteData`.
- **The issue's own guidance.** It asks not to add a heavy dependency without
  justification, and says in-memory for the session is sufficient (no
  offline/disk cache). A purpose-built library is not warranted here.

Trade-off accepted: we maintain the cache ourselves. It is deliberately minimal
and covered by the smoke test's request-count assertions. If query needs grow
substantially (optimistic mutation queues, infinite scroll, window-focus
refetch, devtools), revisit TanStack — the `useQuery` seam makes that swap local.

## Architecture

Three pieces, all framework-agnostic except the hook:

### 1. `lib/query_key.ts` — stable, order-independent keys

`stableKey(value)` serializes query inputs to a string that is identical for the
same request shape regardless of object-key order, and regardless of element
order for primitive arrays (filter sets like selected accounts / excluded
categories carry no order). `undefined` collapses to `null` so an omitted
optional keys the same entry as an explicit absent value.

### 2. `lib/query_cache.ts` — the store

A module-level `Map<key, Entry>` plus a subscriber registry.

- **Cache hit.** A *fresh* entry (within its `staleTime`) is served without a
  network call.
- **In-flight dedup.** Concurrent callers for the same key share one `promise`,
  so two consumers requesting the same shape make one request regardless of
  where they sit in the tree.
- **Stale-while-revalidate.** A refetch keeps the previous successful value
  visible until the new one lands.
- **Invalidation.** `invalidateVolatile()` (the default after a write) drops all
  non-static entries; `invalidateAll()` includes static reference data;
  `clearCache()` wipes everything. Entries with active subscribers refetch
  immediately so the visible UI updates; inactive entries are dropped so the
  next mount fetches fresh.
- **Lifetime.** Browser session only. A page reload clears the cache. No disk
  cache by design.

### 3. `hooks/use_query.ts` — the hook

`useQuery(fetcher, { tag, hard, soft, enabled?, staleTime?, static? })` returns
`[RemoteData<T>, refresh]`. It subscribes to the cache via
`useSyncExternalStore` (React-Compiler-safe) and:

- builds the cache key from `tag` + `hard` + `soft` inputs;
- fetches only when `enabled` (default true) — **a disabled query issues no
  request but still serves an already-cached value for free**;
- preserves the hard/soft semantics of the old hook: a `hard` change (identity,
  e.g. `profileId`) clears the previous value (→ `loading`); a `soft` change
  (filter/view, e.g. date range) keeps it visible (→ `reloading`);
- marks `static` entries (reference data: categories, currencies, profiles) as
  session-stable and exempt from volatile invalidation.

### Writes invalidate automatically

`api/client.ts` wraps the API proxy: any method whose name starts with a
mutation verb (`set/create/update/delete/patch/commit/upload/import`)
invalidates the cache once it resolves. Category/currency/profile writes ripple
into every figure, so they invalidate everything; all other writes invalidate
volatile entries only. Reads and side-effect-free calls (`parseDocuments`,
`exportData`) are untouched. This means **no component needs to remember to
invalidate** — editing a holding refreshes the portfolio queries on its own. The
imperative `refresh()` returned by hooks still works for explicit reloads.

Switching mock/live mode calls `clearCache()` (the two return different data for
the same shape).

## The LCA rule (load placement)

> Load a data dependency at the **lowest** component that is a common ancestor of
> all of its consumers — never higher than necessary.

Worked example:

```
A -> B, C
B -> D
C -> E, F

- X is needed by D and F  ->  load X at A   (LCA of D and F)
- Y is needed by E and F  ->  load Y at C   (LCA of E and F), NOT at A
```

Shared-across-subtrees data is hoisted to the shared ancestor and passed down;
data confined to a subtree is loaded at that subtree's root, not globally.

The request-keyed cache makes this rule *forgiving but still worth following*:
because two consumers of the same shape dedupe to one request, misplacing a load
no longer causes duplicate network traffic. But placement still controls **when**
a load fires (demand-driven: a hidden subtree that owns its load issues nothing)
and keeps prop-drilling shallow. Combine the two: place the load at the LCA, gate
it with `enabled` when the LCA renders before the consumer is visible.

### Two shapes of "lowest"

- **Sole consumer in a subtree → load inside that subtree's root.** The fetch is
  gated by mount: render the component only when its tab is active and it issues
  nothing until shown.
- **Consumed across sibling subtrees → load at the shared ancestor, gate with
  `enabled`.** Pass the result down. Gate the fetch to the views/states that
  actually need it.

## Reference implementation: the Portfolio page

`pages/portfolio.tsx` and `pages/portfolio/portfolio_history.tsx`:

- **History** has exactly one consumer (the History view), so the
  `usePortfolioHistoryData` call lives *inside* `PortfolioHistory`. The page
  mounts that component only on the History tab, so the request fires the first
  time History is opened and never before.
- **Summary** is consumed only by the Overview view, loaded at the page and
  gated `enabled: activeView === "overview"`.
- **Accounts** is consumed by the Accounts grid *and* by the drill-down sheet
  that the page renders independently of the active tab — so the page is its LCA.
  Loaded at the page, gated
  `enabled: activeView === "accounts" || selectedAccountId !== null`, so it loads
  for the grid or whenever a sheet is open, and not otherwise.

Result: landing on Overview issues only the Overview requests; opening each tab
issues its request the first time and cache-serves every repeat; an unopened tab
issues zero requests for its data.

The same demand-driven gating is applied to the Investments tabs (overview vs
history) and the Transactions table-vs-charts datasets. Budget needs none: its
single spending-grid feeds both of its views.

## Adding a new query

1. Write a hook in `hooks/data/` that calls `useQuery(fetcher, opts)`.
2. Pick a unique `tag` (the endpoint name is a good choice).
3. Put identity inputs in `hard`, filter/view inputs in `soft`.
4. Set `static: true` only for session-stable reference data.
5. Add an `enabled` parameter and thread it from the page if the data backs a
   hidden tab/panel.
6. You do **not** need to wire up invalidation: the api-client wrapper handles
   it for any standard mutation method. Add a tag to the wrapper's ripple rules
   only if a write needs to invalidate *static* data it doesn't already cover.

## Non-goals (unchanged)

- No backend changes; endpoint semantics and response shapes are untouched.
- No persistent/disk cache — in-memory for the session only.
