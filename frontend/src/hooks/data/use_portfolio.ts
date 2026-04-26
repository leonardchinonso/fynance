import { api } from "@/api/client"
import type { Granularity, Holding, PortfolioResponse, PortfolioHistoryRow, AccountSnapshot, CashFlowMonth } from "@/types"
import type { RemoteData } from "@/lib/remote_data"
import { useRemoteData } from "@/hooks/use_remote_data"

/** All data needed by the portfolio page, fetched as a single unit. */
export interface PortfolioPageData {
  portfolio: PortfolioResponse
  history: PortfolioHistoryRow[]
  accountBalances: AccountSnapshot[]
  cashFlow: CashFlowMonth[]
  /** Holdings for every investment and pension account. */
  allHoldings: Holding[]
}

/**
 * Fetches all portfolio page data in parallel.
 *
 * - Hard dep: `profileId` — switching profile wipes data and shows a skeleton.
 * - Soft deps: `start`, `end`, `granularity` — date/view changes show a reloading overlay.
 *
 * Holdings are fetched for every `investment` or `pension` account returned by the
 * portfolio summary. The two-stage fetch is encapsulated here so the page stays simple.
 */
export function usePortfolio(
  start: string,
  end: string,
  granularity: Granularity,
  profileId: string | undefined,
): RemoteData<PortfolioPageData> {
  const [data] = useRemoteData(
    async () => {
      const [portfolio, history, accountBalances, cashFlow] = await Promise.all([
        api.getPortfolio(profileId),
        api.getPortfolioHistory(start, end, granularity, profileId),
        api.getAccountBalances(start, end, profileId),
        api.getCashFlow(start, end, granularity, profileId),
      ])

      const investmentAccountIds = portfolio.accounts
        .filter(a => a.type === "investment" || a.type === "pension")
        .map(a => a.id)

      const holdingsPerAccount = await Promise.all(
        investmentAccountIds.map(id => api.getHoldings(id))
      )
      const allHoldings = holdingsPerAccount.flat()

      return { portfolio, history, accountBalances, cashFlow, allHoldings }
    },
    { hard: [profileId], soft: [start, end, granularity] },
  )
  return data
}
