import { useCallback, useState } from "react"
import { api } from "@/api/client"
import type { CgtFilters } from "@/api/service"
import type { CapitalGainsResponse } from "@/bindings/CapitalGainsResponse"
import { RemoteData } from "@/lib/remote_data"

/**
 * Imperative CGT report fetcher. The report only runs when the user clicks
 * "Generate", so unlike `useRemoteData` this hook does not auto-fetch on
 * filter changes — the caller drives it.
 */
export function useCapitalGains(): {
  state: RemoteData<CapitalGainsResponse>
  generate: (filters: CgtFilters) => Promise<CapitalGainsResponse>
  reset: () => void
} {
  const [state, setState] = useState<RemoteData<CapitalGainsResponse>>(
    RemoteData.idle<CapitalGainsResponse>()
  )

  const generate = useCallback(async (filters: CgtFilters) => {
    setState((prev) =>
      prev.status === "succeeded"
        ? RemoteData.reloading(prev.value)
        : RemoteData.loading<CapitalGainsResponse>()
    )
    try {
      const response = await api.getCapitalGains(filters)
      setState(RemoteData.succeeded(response))
      return response
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err)
      setState(RemoteData.failed<CapitalGainsResponse>(msg))
      throw err
    }
  }, [])

  const reset = useCallback(() => setState(RemoteData.idle<CapitalGainsResponse>()), [])

  return { state, generate, reset }
}
