import { useState, useEffect, useRef } from "react"
import type { DependencyList } from "react"
import { RemoteData } from "@/lib/remote_data"

/**
 * Fetches async data with hard/soft dependency classification.
 *
 * **Hard deps** — trigger a full reload (transitions to `loading`, clears the
 * previous value). Use for identity changes such as `profileId` switching — showing
 * stale data from the previous identity would be misleading.
 *
 * **Soft deps** — trigger a background reload (transitions to `reloading`, keeps the
 * previous value visible). Use for filter or view changes such as date range or
 * granularity — the old data is still valid context while the new data loads.
 *
 * If a dep appears in both arrays, hard wins.
 *
 * @returns `[data, refresh]` — the current `RemoteData` state plus an imperative
 * `refresh()` function that triggers a soft reload without changing any dep value.
 * Useful for post-mutation cache invalidation.
 */
export function useRemoteData<T>(
  fetcher: () => Promise<T>,
  deps: { hard: DependencyList; soft: DependencyList }
): [RemoteData<T>, () => void] {
  const [state, setState] = useState<RemoteData<T>>(RemoteData.idle<T>())

  // Tracks whether the soft effect should skip (first render is handled by the hard effect)
  const isFirstRender = useRef(true)

  // Incrementing counter for imperative refresh — adding it to soft deps triggers
  // a reloading transition without touching any URL param or filter value.
  const refreshCounter = useRef(0)
  const [refreshTick, setRefreshTick] = useState(0)

  const refresh = () => {
    refreshCounter.current += 1
    setRefreshTick(refreshCounter.current)
  }

  // ── Effect 1: hard deps ────────────────────────────────────────────────────
  // Transitions to `loading` (wipes value). Runs on mount and whenever any hard dep changes.
  useEffect(() => {
    isFirstRender.current = false
    setState(RemoteData.loading<T>())
    let cancelled = false
    fetcher()
      .then(v => { if (!cancelled) setState(RemoteData.succeeded(v)) })
      .catch((e: unknown) => {
        if (!cancelled) {
          const msg = e instanceof Error ? e.message : "Failed to load"
          setState(RemoteData.failed<T>(msg))
        }
      })
    return () => { cancelled = true }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [...deps.hard])

  // ── Effect 2: soft deps ────────────────────────────────────────────────────
  // Transitions to `reloading` (keeps value). Skips on first render since Effect 1
  // already handles that. If a hard dep just changed, state is already `loading` so
  // the guard below prevents a redundant `reloading` transition — hard wins.
  useEffect(() => {
    if (isFirstRender.current) return
    setState(prev => {
      if (prev.status === "succeeded" || prev.status === "reloading")
        return RemoteData.reloading(prev.value)
      if (prev.status === "failed")
        return RemoteData.loading<T>() // retry from failed shows skeleton
      return prev // already loading — no change
    })
    let cancelled = false
    fetcher()
      .then(v => { if (!cancelled) setState(RemoteData.succeeded(v)) })
      .catch((e: unknown) => {
        if (!cancelled) {
          const msg = e instanceof Error ? e.message : "Failed to load"
          setState(RemoteData.failed<T>(msg))
        }
      })
    return () => { cancelled = true }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [...deps.hard, ...deps.soft, refreshTick])

  return [state, refresh]
}
