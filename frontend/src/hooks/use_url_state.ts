import { useCallback } from "react"
import { useSearchParams } from "react-router-dom"

/**
 * Thin wrapper around useSearchParams for "many small params" pages.
 *
 * - `get(key, default)`: returns the current value or the default.
 * - `set({ k: v, ... })`: merges updates into the URL via `replace`. Empty
 *   strings and `null` delete the param so the URL stays tidy.
 *
 * Reads are cheap (just a map lookup); the writer is memoized so callers
 * can put it in effect/callback dep arrays without churning.
 */
export function useUrlState() {
  const [sp, setSp] = useSearchParams()

  const get = useCallback(
    (key: string, fallback: string = ""): string => sp.get(key) ?? fallback,
    [sp]
  )

  const set = useCallback(
    (updates: Record<string, string | null | undefined>) => {
      setSp(
        (prev) => {
          const next = new URLSearchParams(prev)
          for (const [k, v] of Object.entries(updates)) {
            if (v == null || v === "") next.delete(k)
            else next.set(k, v)
          }
          return next
        },
        { replace: true }
      )
    },
    [setSp]
  )

  return { get, set }
}
