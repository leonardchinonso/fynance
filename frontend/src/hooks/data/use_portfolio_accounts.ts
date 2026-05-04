import { api } from "@/api/client"
import type { Account, AccountSnapshot, Currency } from "@/types"
import type { RemoteData } from "@/lib/remote_data"
import { useRemoteData } from "@/hooks/use_remote_data"

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
 */
export function usePortfolioAccounts(
  start: string,
  end: string,
  profileId: string | undefined,
): RemoteData<PortfolioAccountsData> {
  const [data] = useRemoteData(
    async () => {
      const [portfolioResponse, accountBalances, currencies] = await Promise.all([
        api.getPortfolio(profileId),
        api.getAccountBalances(start, end, profileId),
        api.getCurrencies(),
      ])
      return { accounts: portfolioResponse.accounts, accountBalances, currencies }
    },
    { hard: [profileId], soft: [start, end] },
  )
  return data
}
