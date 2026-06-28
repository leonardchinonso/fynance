/**
 * In-memory, request-keyed cache for idempotent (GET) endpoints.
 *
 * Keyed by request shape (endpoint tag + a stable serialization of its inputs,
 * see {@link stableKey}). A fresh cache hit is served without a network call, so
 * navigating away from a view and back with the same request shape does not
 * refetch. In-flight requests for the same key are deduplicated: two consumers
 * asking for the same shape share a single network request regardless of where
 * they sit in the tree.
 *
 * Lifetime is the browser session (a page reload clears everything). There is no
 * persistent/disk cache by design — see the issue's non-goals.
 *
 * Freshness is governed per entry by `staleTime`; mutations invalidate entries
 * explicitly (see {@link invalidateVolatile} / {@link invalidateAll}). The
 * `api` client wires those into every write — see `api/client.ts`.
 */

type EntryStatus =
  | { kind: "loading" }
  | { kind: "success"; value: unknown; updatedAt: number }
  | { kind: "error"; error: string; updatedAt: number }

interface Entry {
  status: EntryStatus
  /** In-flight request, if any. Used to deduplicate concurrent callers. */
  promise?: Promise<unknown>
  /** Last fetcher seen for this key, so invalidation can re-run it. */
  fetcher?: () => Promise<unknown>
  staleTime: number
  /** Session-stable reference data (categories, currencies, profiles). Excluded
   *  from {@link invalidateVolatile}. */
  isStatic: boolean
}

const store = new Map<string, Entry>()
const listeners = new Map<string, Set<() => void>>()

/**
 * Dev-only per-key counter of *real* fetches (cache misses that actually hit the
 * fetcher — not cache hits or deduped joins). Exposed on `window.__queryFetchCounts`
 * so the Playwright smoke test can assert demand-driven + cache behaviour. Compiled
 * out of production builds.
 */
function recordFetch(key: string): void {
  if (!import.meta.env.DEV) return
  const w = globalThis as unknown as { __queryFetchCounts?: Record<string, number> }
  w.__queryFetchCounts ??= {}
  w.__queryFetchCounts[key] = (w.__queryFetchCounts[key] ?? 0) + 1
}

function emit(key: string): void {
  const set = listeners.get(key)
  if (set) for (const cb of set) cb()
}

/** Subscribe to changes for one key. Returns an unsubscribe function. */
export function subscribe(key: string, cb: () => void): () => void {
  let set = listeners.get(key)
  if (!set) {
    set = new Set()
    listeners.set(key, set)
  }
  set.add(cb)
  return () => {
    set.delete(cb)
    if (set.size === 0) listeners.delete(key)
  }
}

/** Current cache entry for a key, or `undefined` if never fetched. */
export function getEntry(key: string): Entry | undefined {
  return store.get(key)
}

function isFresh(entry: Entry | undefined): boolean {
  return (
    entry?.status.kind === "success" &&
    Date.now() - entry.status.updatedAt < entry.staleTime
  )
}

interface FetchOptions {
  staleTime: number
  isStatic: boolean
  /** Bypass freshness and in-flight dedup, forcing a new request (refresh / invalidation). */
  force?: boolean
}

/**
 * Resolve a query: serve a fresh cached value, join an in-flight request, or
 * start a new one. The resulting value is written back to the cache and
 * subscribers are notified.
 */
export function fetchQuery<T>(
  key: string,
  fetcher: () => Promise<T>,
  opts: FetchOptions,
): Promise<T> {
  const existing = store.get(key)

  if (!opts.force) {
    if (existing?.promise) return existing.promise as Promise<T>
    if (existing && isFresh(existing)) {
      return Promise.resolve((existing.status as { value: T }).value)
    }
  }

  recordFetch(key)
  const promise = fetcher().then(
    (value) => {
      const current = store.get(key)
      // A later force-refresh may have superseded this request; only commit if
      // our promise is still the active one.
      if (current && current.promise && current.promise !== promise) return value
      store.set(key, {
        status: { kind: "success", value, updatedAt: Date.now() },
        fetcher: fetcher as () => Promise<unknown>,
        staleTime: opts.staleTime,
        isStatic: opts.isStatic,
      })
      emit(key)
      return value
    },
    (err: unknown) => {
      const current = store.get(key)
      if (current && current.promise && current.promise !== promise) throw err
      const message = err instanceof Error ? err.message : "Failed to load"
      store.set(key, {
        status: { kind: "error", error: message, updatedAt: Date.now() },
        fetcher: fetcher as () => Promise<unknown>,
        staleTime: opts.staleTime,
        isStatic: opts.isStatic,
      })
      emit(key)
      throw err
    },
  )

  // Preserve a previous successful value while refetching (stale-while-revalidate)
  // so consumers can show the old value rather than a spinner.
  const base: Entry =
    existing?.status.kind === "success"
      ? { ...existing, promise, fetcher: fetcher as () => Promise<unknown>, staleTime: opts.staleTime, isStatic: opts.isStatic }
      : { status: { kind: "loading" }, promise, fetcher: fetcher as () => Promise<unknown>, staleTime: opts.staleTime, isStatic: opts.isStatic }
  store.set(key, base)
  emit(key)

  return promise
}

/**
 * Invalidate every entry matching `predicate`. Entries with active subscribers
 * are refetched immediately (so the visible UI updates); inactive entries are
 * dropped so the next mount fetches fresh.
 */
export function invalidate(predicate: (entry: Entry, key: string) => boolean): void {
  for (const [key, entry] of [...store]) {
    if (!predicate(entry, key)) continue
    const active = (listeners.get(key)?.size ?? 0) > 0
    if (active && entry.fetcher) {
      fetchQuery(key, entry.fetcher, {
        staleTime: entry.staleTime,
        isStatic: entry.isStatic,
        force: true,
      }).catch(() => {})
    } else {
      store.delete(key)
    }
  }
}

/** Invalidate all non-static entries. The default after a write. */
export function invalidateVolatile(): void {
  invalidate((entry) => !entry.isStatic)
}

/** Invalidate everything, including static reference data (categories, FX, profiles). */
export function invalidateAll(): void {
  invalidate(() => true)
}

/** Drop the entire cache and notify every subscriber (e.g. on mock/live mode switch). */
export function clearCache(): void {
  const keys = [...store.keys(), ...listeners.keys()]
  store.clear()
  for (const key of keys) emit(key)
}
