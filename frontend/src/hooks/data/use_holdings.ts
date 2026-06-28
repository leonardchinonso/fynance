import { api } from "@/api/client"
import type { Holding } from "@/types"
import type { RemoteData } from "@/lib/remote_data"
import { useQuery } from "@/hooks/use_query"

/**
 * Fetches holdings for a single account. Returns `idle` when `accountId` is null
 * (e.g. when no account is selected in the drill-down sheet) — the query is
 * disabled, so no request is issued.
 *
 * - Hard dep: `accountId` — switching accounts wipes holdings and shows a skeleton.
 */
export function useHoldings(accountId: string | null): RemoteData<Holding[]> {
  const [data] = useQuery(
    () => api.getHoldings(accountId as string),
    { tag: "holdings", hard: [accountId], soft: [], enabled: accountId !== null },
  )
  return data
}
