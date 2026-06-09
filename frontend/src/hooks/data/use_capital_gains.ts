import { useCallback, useState } from "react"
import { api } from "@/api/client"
import type { CgtFilters } from "@/api/service"
import type { CapitalGainsResponse } from "@/bindings/CapitalGainsResponse"
import { RemoteData } from "@/lib/remote_data"

/**
 * Imperative CGT report fetcher. The report only runs when the user clicks
 * "Generate", so unlike `useRemoteData` this hook does not auto-fetch on
 * filter changes — the caller drives it.
 *
 * `error` is preserved alongside `state` so callers can branch on `ApiError`
 * codes (e.g. show a "configure missing currencies" CTA on
 * `code: "missing_currencies"`) without parsing the message string.
 */
export function useCapitalGains(): {
  state: RemoteData<CapitalGainsResponse>
  error: Error | null
  generate: (filters: CgtFilters) => Promise<CapitalGainsResponse>
  reset: () => void
} {
  const [state, setState] = useState<RemoteData<CapitalGainsResponse>>(
    RemoteData.idle<CapitalGainsResponse>()
  )
  const [error, setError] = useState<Error | null>(null)

  const generate = useCallback(async (filters: CgtFilters) => {
    setState((prev) =>
      prev.status === "succeeded"
        ? RemoteData.reloading(prev.value)
        : RemoteData.loading<CapitalGainsResponse>()
    )
    setError(null)
    try {
      const response = await api.getCapitalGains(filters)
      setState(RemoteData.succeeded(response))
      return response
    } catch (err) {
      const wrapped = err instanceof Error ? err : new Error(String(err))
      setError(wrapped)
      setState(RemoteData.failed<CapitalGainsResponse>(wrapped.message))
      throw err
    }
  }, [])

  const reset = useCallback(() => {
    setState(RemoteData.idle<CapitalGainsResponse>())
    setError(null)
  }, [])

  return { state, error, generate, reset }
}
