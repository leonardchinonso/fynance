import { api } from "@/api/client"
import type { Account } from "@/types"
import type { RemoteData } from "@/lib/remote_data"
import { useQuery } from "@/hooks/use_query"

/** Accounts and category names available for filter dropdowns. */
export interface FilterOptions {
  accounts: Account[]
  categories: string[]
}

/**
 * Fetches filter dropdown options (accounts + categories) once per profile.
 * These populate the MultiSelect filters on the Transactions page.
 *
 * - Hard dep: `profileId` — re-fetches when the profile changes.
 * - No soft deps — not date-range dependent.
 */
export function useFilterOptions(
  profileId: string | undefined,
): RemoteData<FilterOptions> {
  const [data] = useQuery(
    async () => {
      const [accounts, categories] = await Promise.all([
        api.getAccounts(profileId),
        api.getCategories(),
      ])
      return { accounts, categories }
    },
    { tag: "filter-options", hard: [profileId], soft: [] },
  )
  return data
}
