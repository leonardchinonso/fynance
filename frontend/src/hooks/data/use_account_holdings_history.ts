import { api } from "@/api/client"
import type { AccountHoldingsHistory } from "@/api/service"
import type { Granularity } from "@/types"
import type { RemoteData } from "@/lib/remote_data"
import { useQuery } from "@/hooks/use_query"

/**
 * Fetches the per-holding value history for a single account. Returns `idle`
 * when `accountId` is null (e.g. when the drill-down sheet is closed) — the
 * query is disabled, so no request is issued.
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
  const [data] = useQuery(
    () => api.getAccountHoldingsHistory(accountId as string, start, end, granularity),
    {
      tag: "account-holdings-history",
      hard: [accountId],
      soft: [start, end, granularity],
      enabled: accountId !== null,
    },
  )
  return data
}
