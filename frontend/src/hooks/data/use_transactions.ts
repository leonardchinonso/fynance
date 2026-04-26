import { api } from "@/api/client"
import type { CategoryTotal, CategoryTotalFilters, PaginatedResponse, Transaction, TransactionFilters } from "@/types"
import type { RemoteData } from "@/lib/remote_data"
import { useRemoteData } from "@/hooks/use_remote_data"

/** Transaction rows plus a map of accountId → display name for the table. */
export interface TransactionsData {
  result: PaginatedResponse<Transaction>
  accountNameMap: Record<string, string>
}

/**
 * Fetches paginated transaction rows and an account name map in parallel.
 *
 * - Hard dep: `profileId`
 * - Soft deps: all filter values (date range, accounts, categories, search, pagination)
 */
export function useTransactions(
  start: string,
  end: string,
  selectedAccounts: string[],
  selectedCategories: string[],
  search: string,
  page: number,
  pageSize: number,
  profileId: string | undefined,
): RemoteData<TransactionsData> {
  const accountsKey = selectedAccounts.join(",")
  const categoriesKey = selectedCategories.join(",")

  const [data] = useRemoteData(
    async () => {
      const filters: TransactionFilters = {
        start,
        end,
        accounts: selectedAccounts.length > 0 ? selectedAccounts : undefined,
        categories: selectedCategories.length > 0 ? selectedCategories : undefined,
        search: search || undefined,
        page,
        limit: pageSize,
        profile_id: profileId,
      }
      const [result, accounts] = await Promise.all([
        api.getTransactions(filters),
        api.getAccounts(profileId),
      ])
      const accountNameMap: Record<string, string> = {}
      for (const a of accounts) accountNameMap[a.id] = a.name
      return { result, accountNameMap }
    },
    { hard: [profileId], soft: [start, end, accountsKey, categoriesKey, search, page, pageSize] },
  )
  return data
}

/** Fetches per-category spending totals for charts. */
export function useTransactionCharts(
  start: string,
  end: string,
  selectedAccounts: string[],
  selectedCategories: string[],
  profileId: string | undefined,
): RemoteData<CategoryTotal[]> {
  const accountsKey = selectedAccounts.join(",")
  const categoriesKey = selectedCategories.join(",")

  const [data] = useRemoteData(
    () => {
      const filters: CategoryTotalFilters = {
        start,
        end,
        accounts: selectedAccounts.length > 0 ? selectedAccounts : undefined,
        categories: selectedCategories.length > 0 ? selectedCategories : undefined,
        profile_id: profileId,
        direction: "outflow",
      }
      return api.getTransactionsByCategory(filters)
    },
    { hard: [profileId], soft: [start, end, accountsKey, categoriesKey] },
  )
  return data
}
