import { api } from "@/api/client"
import type { AccountHoldingsHistory } from "@/api/service"
import type { Granularity } from "@/types"
import { RemoteData } from "@/lib/remote_data"
import { useRemoteData } from "@/hooks/use_remote_data"

/**
 * Fetches the per-holding value history for a single account. Returns `idle`
 * when `accountId` is null (e.g. when the drill-down sheet is closed).
 *
 * - Hard dep: `accountId` — switching accounts wipes the chart and shows a skeleton.
 * - Soft deps: `start`, `end`, `granularity` — re-fetch in place via reloading.
 */
export function useAccountHoldingsHistory(
  accountId: string | null,
  start: string,
  end: string,
  granularity: Granularity,
): RemoteData<AccountHoldingsHistory> {
  const [data] = useRemoteData(
    () => {
      if (!accountId) {
        return Promise.resolve({ preferred_currency: "GBP", symbols: [], rows: [] })
      }
      return api.getAccountHoldingsHistory(accountId, start, end, granularity)
    },
    { hard: [accountId], soft: [start, end, granularity] },
  )

  if (accountId === null) return RemoteData.idle<AccountHoldingsHistory>()
  return data
}
