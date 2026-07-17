import { api } from "@/api/client"
import type { Account, AccountSnapshot, Currency } from "@/types"
import type { RemoteData } from "@/lib/remote_data"
import { combineRemoteData } from "@/lib/remote_data"
import { useQuery } from "@/hooks/use_query"

/** Data needed by the Accounts view. */
export interface PortfolioAccountsData {
  accounts: Account[]
  accountBalances: AccountSnapshot[]
  currencies: Currency[]
}

/**
 * Composes the Accounts view data set from per-endpoint cache entries.
 *
 * - Hard dep: `profileId`
 * - Soft deps: `start`, `end`
 *
 * `enabled` gates the fetches so the data loads only when the Accounts view
 * (or the account drill-down sheet, which also consumes it) is shown.
 */
export function usePortfolioAccounts(
  start: string,
  end: string,
  profileId: string | undefined,
  enabled = true,
): RemoteData<PortfolioAccountsData> {
  // Balances as of the range end date: the most recent snapshot on/before
  // `end` (carry-forward), so a past end date shows historical balances
  // rather than today's.
  const [portfolio] = useQuery(
    () => api.getPortfolio(profileId, end),
    { tag: "holdings-summary", hard: [profileId], soft: [end], enabled },
  )
  const [balances] = useQuery(
    () => api.getAccountBalances(start, end),
    { tag: "holdings-balances", hard: [], soft: [start, end], enabled },
  )
  const [currencies] = useQuery(
    () => api.getCurrencies(),
    { tag: "currencies", hard: [], soft: [], static: true, enabled },
  )

  return combineRemoteData(
    [portfolio, balances, currencies] as const,
    ([p, accountBalances, c]) => ({
      accounts: p.accounts,
      accountBalances,
      currencies: c,
    }),
  )
}
