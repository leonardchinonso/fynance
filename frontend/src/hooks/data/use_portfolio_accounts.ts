import { api } from "@/api/client"
import type { Account, AccountSnapshot, Currency } from "@/types"
import type { RemoteData } from "@/lib/remote_data"
import { useQuery } from "@/hooks/use_query"

/** Data needed by the Accounts view. */
export interface PortfolioAccountsData {
  accounts: Account[]
  accountBalances: AccountSnapshot[]
  currencies: Currency[]
}

/**
 * Fetches accounts and per-account balance snapshots for the Accounts view.
 *
 * - Hard dep: `profileId`
 * - Soft deps: `start`, `end`
 *
 * `enabled` gates the fetch so the data loads only when the Accounts view (or
 * the account drill-down sheet, which also consumes it) is shown.
 */
export function usePortfolioAccounts(
  start: string,
  end: string,
  profileId: string | undefined,
  enabled = true,
): RemoteData<PortfolioAccountsData> {
  const [data] = useQuery(
    async () => {
      const [portfolioResponse, accountBalances, currencies] = await Promise.all([
        api.getPortfolio(profileId),
        api.getAccountBalances(start, end, profileId),
        api.getCurrencies(),
      ])
      return { accounts: portfolioResponse.accounts, accountBalances, currencies }
    },
    { tag: "portfolio-accounts", hard: [profileId], soft: [start, end], enabled },
  )
  return data
}
