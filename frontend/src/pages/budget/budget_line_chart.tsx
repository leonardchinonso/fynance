import type { SpendingGridRow, Granularity } from "@/types"
import type { CategoryType } from "@/bindings/CategoryType"
import { StyledLineChart } from "@/components/charts"
import { formatPeriodKey, periodKeysFromRows, categoryParent } from "@/lib/utils"
import { CATEGORY_COLORS } from "@/lib/colors"
import { groupLabelForType } from "@/lib/category_types"
import { useCategoryColorsContext } from "@/context/category_colors_context"
import { useCategoryMeta } from "@/context/category_names_context"
import { useThemeContext } from "@/context/theme_context"

interface BudgetLineChartProps {
  rows: SpendingGridRow[]
  months: string[]
  granularity: Granularity
  groupBy: string
  accountNameMap: Record<string, string>
}

const PALETTE = Object.values(CATEGORY_COLORS)
const NEUTRAL = "#78716c"
const TOTAL_LABEL = "Total"
// Muted gray so the aggregate overlay stays subtle in either theme rather than
// a bright white line dominating the chart in dark mode.
const TOTAL_COLOR = { dark: "#6b7280", light: "#9ca3af" }

export function BudgetLineChart({ rows, granularity, groupBy, accountNameMap }: BudgetLineChartProps) {
  const { categoryColors } = useCategoryColorsContext()
  const { resolve, parentName } = useCategoryMeta()
  const { resolvedTheme } = useThemeContext()

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
  const baseCategories = Array.from(new Set(labels)).slice(0, 8)
  const shown = new Set(baseCategories)

  const periodKeys = periodKeysFromRows(rows)

  // Total sums every filtered row per period, including categories beyond the
  // top-8 drawn as individual lines, so it reflects the full filtered spend.
  const data = periodKeys.map((p) => {
    const entry: Record<string, string | number> = {
      period: formatPeriodKey(p, granularity),
    }
    let total = 0
    rows.forEach((row, i) => {
      const n = parseFloat(row.periods[p] ?? "")
      const value = Number.isFinite(n) ? Math.abs(n) : 0
      total += value
      const label = labels[i]
      if (shown.has(label)) entry[label] = ((entry[label] as number) ?? 0) + value
    })
    entry[TOTAL_LABEL] = total
    return entry
  })

  const colorByParent = (name: string) =>
    categoryColors[name] ?? CATEGORY_COLORS[name] ?? NEUTRAL
  const baseColors = baseCategories.map((label, i) => {
    if (groupBy === "leaf_category") return colorByParent(categoryParent(label))
    if (groupBy === "parent_category") return colorByParent(label)
    return PALETTE[i % PALETTE.length] ?? NEUTRAL
  })

  const categories = [TOTAL_LABEL, ...baseCategories]
  const colors = [TOTAL_COLOR[resolvedTheme], ...baseColors]

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
        dashedKeys={[TOTAL_LABEL]}
        height={340}
        curved
      />
    </div>
  )
}
