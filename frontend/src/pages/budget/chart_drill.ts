import type { SpendingGridRow } from "@/types"
import { expandGroups, groupsForTypes } from "@/lib/category_types"

/**
 * URL-param patch that filters the Transactions table to the clicked chart
 * series, given the chart's `groupBy` and how it labels a row. Returns null when
 * the series can't be mapped to a filter (e.g. a parent with no children).
 *
 * - leaf_category  → that leaf category id
 * - parent_category→ all leaf ids under the parent (transactions are leaf-tagged)
 * - category_type  → all raw types in the user-facing group
 * - account        → that account id
 */
export function categoryFilterForSeries(
  rows: SpendingGridRow[],
  label: string,
  groupBy: string,
  seriesLabel: (r: SpendingGridRow) => string,
  childIdsOf: (parentId: string) => string[],
): Record<string, string | undefined> | null {
  const row = rows.find((r) => seriesLabel(r) === label)
  if (!row) return null
  switch (groupBy) {
    case "leaf_category":
      return row.category_id ? { categories: row.category_id } : null
    case "category_type":
      return row.group_key
        ? { category_types: expandGroups(groupsForTypes([row.group_key])).join(",") }
        : null
    case "account":
      return row.group_key ? { accounts: row.group_key } : null
    default: {
      const kids = row.group_key ? childIdsOf(row.group_key) : []
      return kids.length ? { categories: kids.join(",") } : null
    }
  }
}
