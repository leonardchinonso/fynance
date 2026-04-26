import { api } from "@/api/client"
import type { Granularity, PortfolioHistoryRow } from "@/types"
import type { RemoteData } from "@/lib/remote_data"
import { useRemoteData } from "@/hooks/use_remote_data"

/**
 * Fetches portfolio history rows for the History view.
 *
 * Named `usePortfolioHistoryData` to avoid collision with the
 * `getPortfolioHistory` API method.
 *
 * - Hard dep: `profileId`
 * - Soft deps: `start`, `end`, `granularity`
 */
export function usePortfolioHistoryData(
  start: string,
  end: string,
  granularity: Granularity,
  profileId: string | undefined,
): RemoteData<PortfolioHistoryRow[]> {
  const [data] = useRemoteData(
    () => api.getPortfolioHistory(start, end, granularity, profileId),
    { hard: [profileId], soft: [start, end, granularity] },
  )
  return data
}
