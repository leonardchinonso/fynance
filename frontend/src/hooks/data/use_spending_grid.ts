import { api } from "@/api/client"
import type { Granularity, SpendingGridFilters, SpendingGridRow } from "@/types"
import type { RemoteData } from "@/lib/remote_data"
import { useQuery } from "@/hooks/use_query"

/**
 * Fetches the spending grid (budget spreadsheet rows / chart series).
 *
 * - Hard dep: `profileId` — switching profile wipes the grid and shows a skeleton.
 * - Soft deps: `start`, `end`, `granularity`, and the filter/grouping values —
 *   changes keep the old grid visible while new data loads.
 *
 * `filters` carries account/category/category-type filters and the `groupBy`
 * dimension (leaf_category | parent_category | category_type | account).
 * `enabled` gates the fetch so a hidden view issues no request.
 *
 * Returns `[data, refresh]` — call `refresh()` after a budget mutation to reload.
 */
export function useSpendingGrid(
  start: string,
  end: string,
  granularity: Granularity,
  profileId: string | undefined,
  filters: SpendingGridFilters = {},
  enabled = true,
): [RemoteData<SpendingGridRow[]>, () => void] {
  // Sorted so the cache key is order-insensitive (same selection in a
  // different order must not refetch).
  const accountsKey = [...(filters.accounts ?? [])].sort().join(",")
  const categoriesKey = [...(filters.categories ?? [])].sort().join(",")
  const typesKey = [...(filters.categoryTypes ?? [])].sort().join(",")
  const groupBy = filters.groupBy ?? ""
  return useQuery(
    () => api.getSpendingGrid(start, end, granularity, profileId, filters),
    {
      tag: "spending-grid",
      hard: [profileId],
      soft: [start, end, granularity, accountsKey, categoriesKey, typesKey, groupBy],
      enabled,
    },
  )
}
