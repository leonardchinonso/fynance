import { api } from "@/api/client"
import type { Currency, Granularity, Holding, PortfolioHistoryRow, PortfolioResponse } from "@/types"
import type { RemoteData } from "@/lib/remote_data"
import { useQuery } from "@/hooks/use_query"

/** Data needed by the Overview and Charts views. */
export interface PortfolioSummaryData {
  portfolio: PortfolioResponse
  /** Used in Overview for start/end net worth delta over the selected period. */
  history: PortfolioHistoryRow[]
  /** Holdings for all accounts (empty array for accounts with no positions). */
  allHoldings: Holding[]
  /** FX rates keyed by currency code, for converting holding values. */
  currencies: Currency[]
}

/**
 * Fetches portfolio summary, history, and holdings in parallel.
 * Used by the Overview and Charts views.
 *
 * - Hard dep: `profileId`
 * - Soft deps: `start`, `end`, `granularity`
 *
 * `enabled` gates the fetch so the Overview tab issues no request until shown.
 */
export function usePortfolioSummary(
  start: string,
  end: string,
  granularity: Granularity,
  profileId: string | undefined,
  enabled = true,
): RemoteData<PortfolioSummaryData> {
  const [data] = useQuery(
    async () => {
      const [portfolio, history, currencies] = await Promise.all([
        api.getPortfolio(profileId),
        api.getPortfolioHistory(start, end, granularity, profileId),
        api.getCurrencies(),
      ])

      const allAccountIds = portfolio.accounts.map(a => a.id)
      const allHoldings = allAccountIds.length > 0
        ? await api.getHoldingsBatch(allAccountIds)
        : []

      return { portfolio, history, allHoldings, currencies }
    },
    { tag: "portfolio-summary", hard: [profileId], soft: [start, end, granularity], enabled },
  )
  return data
}
