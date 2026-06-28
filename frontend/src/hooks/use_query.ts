import { useCallback, useEffect, useRef, useSyncExternalStore } from "react"
import type { DependencyList } from "react"
import { RemoteData } from "@/lib/remote_data"
import { stableKey } from "@/lib/query_key"
import { fetchQuery, getEntry, subscribe } from "@/lib/query_cache"

/** Default freshness window for volatile (non-static) queries. */
const DEFAULT_STALE_MS = 5 * 60_000

export interface QueryOptions {
  /**
   * Endpoint identifier. Combined with `hard` + `soft` it forms the cache key,
   * so two queries with the same `tag` and inputs share one entry (and one
   * network request). Mutations invalidate by static/volatile class, not tag.
   */
  tag: string
  /**
   * Identity inputs. A change clears the previously shown value (transition to
   * `loading`) — use for things like `profileId` where showing stale data from
   * the previous identity would mislead.
   */
  hard: DependencyList
  /**
   * Filter/view inputs. A change keeps the previous value visible while the new
   * data loads (transition to `reloading`) — use for date range, granularity, etc.
   */
  soft: DependencyList
  /**
   * When false, the query does not fetch (a hidden tab issues zero requests).
   * An already-cached value is still served for free. Defaults to true.
   */
  enabled?: boolean
  /** Freshness window in ms. Defaults to {@link DEFAULT_STALE_MS}, or `Infinity` when `static`. */
  staleTime?: number
  /** Session-stable reference data; excluded from volatile (default) invalidation. */
  static?: boolean
}

/**
 * Demand-driven, request-keyed data hook built on the in-memory query cache.
 *
 * Drop-in replacement for `useRemoteData` that adds caching, in-flight
 * deduplication, an `enabled` gate, and explicit invalidation, while preserving
 * the `RemoteData` contract and the hard/soft (loading vs reloading) semantics.
 *
 * @returns `[data, refresh]` — `refresh()` forces a refetch ignoring freshness.
 */
export function useQuery<T>(
  fetcher: () => Promise<T>,
  opts: QueryOptions,
): [RemoteData<T>, () => void] {
  const enabled = opts.enabled ?? true
  const isStatic = opts.static ?? false
  const staleTime = opts.staleTime ?? (isStatic ? Infinity : DEFAULT_STALE_MS)

  const key = `${opts.tag}::${stableKey([...opts.hard, ...opts.soft])}`
  const identity = `${opts.tag}::${stableKey([...opts.hard])}`

  const sub = useCallback((cb: () => void) => subscribe(key, cb), [key])
  const snapshot = useCallback(() => getEntry(key), [key])
  const entry = useSyncExternalStore(sub, snapshot)

  // Latest fetcher without retriggering the effect: inputs are encoded in `key`,
  // so the effect only needs to run when the key (or enablement) changes.
  const fetcherRef = useRef(fetcher)
  fetcherRef.current = fetcher

  useEffect(() => {
    if (!enabled) return
    fetchQuery(key, () => fetcherRef.current(), { staleTime, isStatic }).catch(() => {})
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, enabled])

  // Last value this instance showed, tagged with the identity it belonged to, so
  // a soft (same-identity) input change keeps it visible while the new key loads.
  const lastShown = useRef<{ identity: string; value: T } | null>(null)

  const refresh = useCallback(() => {
    fetchQuery(key, () => fetcherRef.current(), { staleTime, isStatic, force: true }).catch(() => {})
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key])

  const status = entry?.status
  if (status?.kind === "success") {
    const value = status.value as T
    lastShown.current = { identity, value }
    return [RemoteData.succeeded(value), refresh]
  }
  if (status?.kind === "error") {
    return [RemoteData.failed<T>(status.error), refresh]
  }
  // loading or not-yet-requested
  const prev = lastShown.current
  if (prev && prev.identity === identity) {
    return [RemoteData.reloading(prev.value), refresh]
  }
  if (!enabled) {
    return [RemoteData.idle<T>(), refresh]
  }
  return [RemoteData.loading<T>(), refresh]
}
