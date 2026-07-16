import { api } from "@/api/client"
import type { CategoryTotal, CategoryTotalFilters, Paginated, SortDir, Transaction, TransactionFilters, TransactionSortColumn } from "@/types"
import type { RemoteData } from "@/lib/remote_data"
import { combineRemoteData } from "@/lib/remote_data"
import { useQuery } from "@/hooks/use_query"

/** Transaction rows plus a map of accountId → display name for the table. */
export interface TransactionsData {
  result: Paginated<Transaction>
  accountNameMap: Record<string, string>
}

/**
 * Composes paginated transaction rows with the account name map from
 * per-endpoint cache entries: page flips refetch only the rows, and the
 * `accounts` entry is shared with every other consumer of the same shape.
 *
 * - Hard dep: `profileId`
 * - Soft deps: all filter values (date range, accounts, categories, search, pagination, sort)
 *
 * `enabled` gates the fetch so the Table view issues no request until shown.
 */
export function useTransactions(
  start: string,
  end: string,
  selectedAccounts: string[],
  selectedCategories: string[],
  selectedCategoryTypes: string[],
  search: string,
  page: number,
  pageSize: number,
  profileId: string | undefined,
  sort: TransactionSortColumn | undefined,
  sortDir: SortDir,
  enabled = true,
): RemoteData<TransactionsData> {
  const accountsKey = selectedAccounts.join(",")
  const categoriesKey = selectedCategories.join(",")
  const typesKey = selectedCategoryTypes.join(",")

  const [result] = useQuery(
    () => {
      const filters: TransactionFilters = {
        start,
        end,
        accounts: selectedAccounts.length > 0 ? selectedAccounts : undefined,
        categories: selectedCategories.length > 0 ? selectedCategories : undefined,
        category_types: selectedCategoryTypes.length > 0 ? selectedCategoryTypes : undefined,
        search: search || undefined,
        page,
        limit: pageSize,
        profile_id: profileId,
        sort,
        sort_dir: sort ? sortDir : undefined,
      }
      return api.getTransactions(filters)
    },
    {
      tag: "transactions",
      hard: [profileId],
      soft: [start, end, accountsKey, categoriesKey, typesKey, search, page, pageSize, sort ?? "", sortDir],
      enabled,
    },
  )
  const [accounts] = useQuery(
    () => api.getAccounts(profileId),
    { tag: "accounts", hard: [profileId], soft: [], enabled },
  )

  return combineRemoteData([result, accounts] as const, ([rows, accountList]) => {
    const accountNameMap: Record<string, string> = {}
    for (const a of accountList) accountNameMap[a.id] = a.name
    return { result: rows, accountNameMap }
  })
}

/** Fetches per-category spending totals for charts. */
export function useTransactionCharts(
  start: string,
  end: string,
  selectedAccounts: string[],
  selectedCategories: string[],
  selectedCategoryTypes: string[],
  profileId: string | undefined,
  enabled = true,
): RemoteData<CategoryTotal[]> {
  const accountsKey = selectedAccounts.join(",")
  const categoriesKey = selectedCategories.join(",")
  const typesKey = selectedCategoryTypes.join(",")

  const [data] = useQuery(
    () => {
      const filters: CategoryTotalFilters = {
        start,
        end,
        accounts: selectedAccounts.length > 0 ? selectedAccounts : undefined,
        categories: selectedCategories.length > 0 ? selectedCategories : undefined,
        category_types: selectedCategoryTypes.length > 0 ? selectedCategoryTypes : undefined,
        profile_id: profileId,
        direction: "outflow",
      }
      return api.getTransactionsByCategory(filters)
    },
    { tag: "transactions-by-category", hard: [profileId], soft: [start, end, accountsKey, categoriesKey, typesKey], enabled },
  )
  return data
}
