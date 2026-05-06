import { api } from "@/api/client"
import type { CashFlowMonth, Currency, Granularity, Holding, PortfolioHistoryRow, PortfolioResponse } from "@/types"
import type { RemoteData } from "@/lib/remote_data"
import { useRemoteData } from "@/hooks/use_remote_data"

/** Data needed by the Overview and Charts views. */
export interface PortfolioSummaryData {
  portfolio: PortfolioResponse
  /** Used in Overview for start/end net worth delta over the selected period. */
  history: PortfolioHistoryRow[]
  cashFlow: CashFlowMonth[]
  /** Holdings for all accounts (empty array for accounts with no positions). */
  allHoldings: Holding[]
  /** FX rates keyed by currency code, for converting holding values. */
  currencies: Currency[]
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
      const [portfolio, history, cashFlow, currencies] = await Promise.all([
        api.getPortfolio(profileId),
        api.getPortfolioHistory(start, end, granularity, profileId),
        api.getCashFlow(start, end, granularity, profileId),
        api.getCurrencies(),
      ])

      const allAccountIds = portfolio.accounts.map(a => a.id)

      const holdingsPerAccount = await Promise.all(
        allAccountIds.map(id => api.getHoldings(id))
      )
      const allHoldings = holdingsPerAccount.flat()

      return { portfolio, history, cashFlow, allHoldings, currencies }
    },
    { hard: [profileId], soft: [start, end, granularity] },
  )
  return data
}
