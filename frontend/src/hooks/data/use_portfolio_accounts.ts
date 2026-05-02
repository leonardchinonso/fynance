import { api } from "@/api/client"
import type { Account, AccountSnapshot } from "@/types"
import type { RemoteData } from "@/lib/remote_data"
import { useRemoteData } from "@/hooks/use_remote_data"

/** Data needed by the Accounts view. */
export interface PortfolioAccountsData {
  accounts: Account[]
  accountBalances: AccountSnapshot[]
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
      const [portfolioResponse, accountBalances] = await Promise.all([
        api.getPortfolio(profileId),
        api.getAccountBalances(start, end, profileId),
      ])
      return { accounts: portfolioResponse.accounts, accountBalances }
    },
    { hard: [profileId], soft: [start, end] },
  )
  return data
}
