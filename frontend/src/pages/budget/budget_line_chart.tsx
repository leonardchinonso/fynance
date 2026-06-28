import type { SpendingGridRow, Granularity } from "@/types"
import type { CategoryType } from "@/bindings/CategoryType"
import { StyledLineChart } from "@/components/charts"
import { formatPeriodKey, periodKeysFromRows, categoryParent } from "@/lib/utils"
import { CATEGORY_COLORS } from "@/lib/colors"
import { CATEGORY_TYPE_LABELS } from "@/bindings/category_type_groups"
import { useCategoryColorsContext } from "@/context/category_colors_context"
import { useCategoryMeta } from "@/context/category_names_context"

interface BudgetLineChartProps {
  rows: SpendingGridRow[]
  months: string[]
  granularity: Granularity
  groupBy: string
  accountNameMap: Record<string, string>
}

const PALETTE = Object.values(CATEGORY_COLORS)
const NEUTRAL = "#78716c"

export function BudgetLineChart({ rows, granularity, groupBy, accountNameMap }: BudgetLineChartProps) {
  const { categoryColors } = useCategoryColorsContext()
  const { resolve, parentName } = useCategoryMeta()

  const seriesLabel = (row: SpendingGridRow): string => {
    switch (groupBy) {
      case "leaf_category":
        return resolve(row.category_id)
      case "category_type":
        return CATEGORY_TYPE_LABELS[row.group_key as CategoryType] ?? row.group_key ?? ""
      case "account":
        return accountNameMap[row.group_key ?? ""] ?? row.group_key ?? ""
      default:
        return parentName(row.group_key)
    }
  }

  const labels = rows.map(seriesLabel)
  const categories = Array.from(new Set(labels)).slice(0, 8)
  const shown = new Set(categories)

  const periodKeys = periodKeysFromRows(rows)

  const data = periodKeys.map((p) => {
    const entry: Record<string, string | number> = {
      period: formatPeriodKey(p, granularity),
    }
    rows.forEach((row, i) => {
      const label = labels[i]
      if (!shown.has(label)) return
      const n = parseFloat(row.periods[p] ?? "")
      const value = Number.isFinite(n) ? Math.abs(n) : 0
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
        Spending Trends
      </h3>
      <StyledLineChart
        data={data}
        index="period"
        categories={categories}
        colors={colors}
        height={340}
        curved
      />
    </div>
  )
}
