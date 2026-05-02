import { api } from "@/api/client"
import type { CashFlowMonth, Granularity, Holding, PortfolioHistoryRow, PortfolioResponse } from "@/types"
import type { RemoteData } from "@/lib/remote_data"
import { useRemoteData } from "@/hooks/use_remote_data"

/** Data needed by the Overview and Charts views. */
export interface PortfolioSummaryData {
  portfolio: PortfolioResponse
  /** Used in Overview for start/end net worth delta over the selected period. */
  history: PortfolioHistoryRow[]
  cashFlow: CashFlowMonth[]
  /** Holdings for all investment + pension accounts. */
  allHoldings: Holding[]
}

/**
 * Fetches portfolio summary, history, cash flow, and holdings in parallel.
 * Used by the Overview and Charts views.
 *
 * - Hard dep: `profileId`
 * - Soft deps: `start`, `end`, `granularity`
 */
export function usePortfolioSummary(
  start: string,
  end: string,
  granularity: Granularity,
  profileId: string | undefined,
): RemoteData<PortfolioSummaryData> {
  const [data] = useRemoteData(
    async () => {
      const [portfolio, history, cashFlow] = await Promise.all([
        api.getPortfolio(profileId),
        api.getPortfolioHistory(start, end, granularity, profileId),
        api.getCashFlow(start, end, granularity, profileId),
      ])

      const investmentAccountIds = portfolio.accounts
        .filter(a => a.type === "investment" || a.type === "pension")
        .map(a => a.id)

      const holdingsPerAccount = await Promise.all(
        investmentAccountIds.map(id => api.getHoldings(id))
      )
      const allHoldings = holdingsPerAccount.flat()

      return { portfolio, history, cashFlow, allHoldings }
    },
    { hard: [profileId], soft: [start, end, granularity] },
  )
  return data
}
