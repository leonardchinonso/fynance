import { api } from "@/api/client"
import type { CategoryTotal, CategoryTotalFilters, Paginated, SortDir, Transaction, TransactionFilters, TransactionSortColumn } from "@/types"
import type { RemoteData } from "@/lib/remote_data"
import { useQuery } from "@/hooks/use_query"

/** Transaction rows plus a map of accountId → display name for the table. */
export interface TransactionsData {
  result: Paginated<Transaction>
  accountNameMap: Record<string, string>
}

/**
 * Fetches paginated transaction rows and an account name map in parallel.
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

  const [data] = useQuery(
    async () => {
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
      const [result, accounts] = await Promise.all([
        api.getTransactions(filters),
        api.getAccounts(profileId),
      ])
      const accountNameMap: Record<string, string> = {}
      for (const a of accounts) accountNameMap[a.id] = a.name
      return { result, accountNameMap }
    },
    {
      tag: "transactions",
      hard: [profileId],
      soft: [start, end, accountsKey, categoriesKey, typesKey, search, page, pageSize, sort ?? "", sortDir],
      enabled,
    },
  )
  return data
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
    { tag: "transaction-charts", hard: [profileId], soft: [start, end, accountsKey, categoriesKey, typesKey], enabled },
  )
  return data
}
