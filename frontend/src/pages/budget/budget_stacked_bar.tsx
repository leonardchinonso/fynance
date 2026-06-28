import type { SpendingGridRow, Granularity } from "@/types"
import type { CategoryType } from "@/bindings/CategoryType"
import { StyledBarChart } from "@/components/charts"
import { formatPeriodKey, periodKeysFromRows, categoryParent } from "@/lib/utils"
import { CATEGORY_COLORS } from "@/lib/colors"
import { groupLabelForType } from "@/lib/category_types"
import { useCategoryColorsContext } from "@/context/category_colors_context"
import { useCategoryMeta } from "@/context/category_names_context"

interface BudgetStackedBarProps {
  rows: SpendingGridRow[]
  months: string[]
  granularity: Granularity
  groupBy: string
  accountNameMap: Record<string, string>
}

const PALETTE = Object.values(CATEGORY_COLORS)
const NEUTRAL = "#78716c"

export function BudgetStackedBar({ rows, granularity, groupBy, accountNameMap }: BudgetStackedBarProps) {
  const { categoryColors } = useCategoryColorsContext()
  const { resolve, parentName } = useCategoryMeta()

  const seriesLabel = (row: SpendingGridRow): string => {
    switch (groupBy) {
      case "leaf_category":
        return resolve(row.category_id)
      case "category_type":
        return row.group_key ? groupLabelForType(row.group_key as CategoryType) : ""
      case "account":
        return accountNameMap[row.group_key ?? ""] ?? row.group_key ?? ""
      default:
        return parentName(row.group_key)
    }
  }

  const labels = rows.map(seriesLabel)
  const categories = Array.from(new Set(labels))

  const periodKeys = periodKeysFromRows(rows)

  const data = periodKeys.map((p) => {
    const entry: Record<string, string | number> = {
      period: formatPeriodKey(p, granularity),
    }
    rows.forEach((row, i) => {
      const n = parseFloat(row.periods[p] ?? "")
      const value = Number.isFinite(n) ? Math.abs(n) : 0
      const label = labels[i]
      entry[label] = ((entry[label] as number) ?? 0) + value
    })
    return entry
  })

  const colorByParent = (name: string) =>
    categoryColors[name] ?? CATEGORY_COLORS[name] ?? NEUTRAL
  const colors = categories.map((label, i) => {
    if (groupBy === "leaf_category") return colorByParent(categoryParent(label))
    if (groupBy === "parent_category") return colorByParent(label)
    return PALETTE[i % PALETTE.length] ?? NEUTRAL
  })

  return (
    <div className="rounded-lg border p-4">
      <h3 className="mb-2 text-sm font-medium text-muted-foreground">
        Spending Over Time
      </h3>
      <StyledBarChart
        data={data}
        index="period"
        categories={categories}
        colors={colors}
        stack
        showTotal
        height={340}
      />
    </div>
  )
}
