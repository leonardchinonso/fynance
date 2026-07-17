import { api } from "@/api/client"
import type { Account } from "@/types"
import type { RemoteData } from "@/lib/remote_data"
import { combineRemoteData } from "@/lib/remote_data"
import { useQuery } from "@/hooks/use_query"

/** Accounts and category names available for filter dropdowns. */
export interface FilterOptions {
  accounts: Account[]
  categories: string[]
}

/**
 * Composes filter dropdown options (accounts + category names) from
 * per-endpoint cache entries shared with the table and settings queries.
 *
 * - Hard dep: `profileId` — re-fetches when the profile changes.
 * - No soft deps — not date-range dependent.
 */
export function useFilterOptions(
  profileId: string | undefined,
): RemoteData<FilterOptions> {
  const [accounts] = useQuery(
    () => api.getAccounts(profileId),
    { tag: "accounts", hard: [profileId], soft: [] },
  )
  const [categories] = useQuery(
    () => api.getCategories(),
    { tag: "category-names", hard: [], soft: [], static: true },
  )

  return combineRemoteData([accounts, categories] as const, ([accountList, names]) => ({
    accounts: accountList,
    categories: names,
  }))
}
