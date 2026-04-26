import { api } from "@/api/client"
import type { Holding } from "@/types"
import { RemoteData } from "@/lib/remote_data"
import { useRemoteData } from "@/hooks/use_remote_data"

/**
 * Fetches holdings for a single account. Returns `idle` when `accountId` is null
 * (e.g. when no account is selected in the drill-down sheet).
 *
 * - Hard dep: `accountId` — switching accounts wipes holdings and shows a skeleton.
 */
export function useHoldings(accountId: string | null): RemoteData<Holding[]> {
  const [data] = useRemoteData(
    () => {
      if (!accountId) return Promise.resolve([])
      return api.getHoldings(accountId)
    },
    { hard: [accountId], soft: [] },
  )

  if (accountId === null) return RemoteData.idle<Holding[]>()
  return data
}
