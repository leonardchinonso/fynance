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
 * `excludeCategoryIds` filters the cash-flow income/spending totals only.
 * Caller is responsible for expanding parent categories to their leaves.
 *
 * - Hard dep: `profileId`
 * - Soft deps: `start`, `end`, `granularity`, `excludeCategoryIds`
 */
export function usePortfolioSummary(
  start: string,
  end: string,
  granularity: Granularity,
  profileId: string | undefined,
  excludeCategoryIds: string[] = [],
): RemoteData<PortfolioSummaryData> {
  const excludeKey = excludeCategoryIds.join(",")
  const [data] = useRemoteData(
    async () => {
      const [portfolio, history, cashFlow, currencies] = await Promise.all([
        api.getPortfolio(profileId),
        api.getPortfolioHistory(start, end, granularity, profileId),
        api.getCashFlow(start, end, granularity, profileId, excludeCategoryIds),
        api.getCurrencies(),
      ])

      const allAccountIds = portfolio.accounts.map(a => a.id)
      const allHoldings = allAccountIds.length > 0
        ? await api.getHoldingsBatch(allAccountIds)
        : []

      return { portfolio, history, cashFlow, allHoldings, currencies }
    },
    { hard: [profileId], soft: [start, end, granularity, excludeKey] },
  )
  return data
}
