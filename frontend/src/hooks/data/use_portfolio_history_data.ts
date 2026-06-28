import { api } from "@/api/client"
import type { Granularity, PortfolioHistoryRow } from "@/types"
import type { RemoteData } from "@/lib/remote_data"
import { useQuery } from "@/hooks/use_query"

/**
 * Fetches portfolio history rows for the History view.
 *
 * Named `usePortfolioHistoryData` to avoid collision with the
 * `getPortfolioHistory` API method.
 *
 * - Hard deps: `profileId`, `granularity`. Granularity is hard because the row
 *   period labels change shape with it ("YYYY-MM" vs "YYYY-Qn" vs "YYYY"); keeping
 *   stale rows of the previous shape while refetching would render quarterly
 *   labels under a monthly formatter (and vice-versa), which throws.
 * - Soft deps: `start`, `end`
 *
 * `enabled` gates the fetch so the History tab issues no request until shown.
 */
export function usePortfolioHistoryData(
  start: string,
  end: string,
  granularity: Granularity,
  profileId: string | undefined,
  enabled = true,
): RemoteData<PortfolioHistoryRow[]> {
  const [data] = useQuery(
    () => api.getPortfolioHistory(start, end, granularity, profileId),
    { tag: "portfolio-history", hard: [profileId, granularity], soft: [start, end], enabled },
  )
  return data
}
