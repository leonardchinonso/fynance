import { api } from "@/api/client"
import type { CashSummaryResponse } from "@/types"
import type { RemoteData } from "@/lib/remote_data"
import { useQuery } from "@/hooks/use_query"

/**
 * Category-type cash summary for the portfolio card: income, spending, savings
 * growth, new cash invested, and range-aware investment metrics.
 *
 * - Hard dep: `profileId`
 * - Soft deps: `start`, `end`
 */
export function useCashSummary(
  start: string,
  end: string,
  profileId: string | undefined,
  enabled = true,
): RemoteData<CashSummaryResponse> {
  const [data] = useQuery(
    () => api.getCashSummary(start, end, profileId),
    { tag: "cash-summary", hard: [profileId], soft: [start, end], enabled },
  )
  return data
}
