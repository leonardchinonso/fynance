import type { SpendingGridRow, Granularity } from "@/types"
import type { CategoryType } from "@/bindings/CategoryType"
import { InteractivePie } from "@/components/charts"
import { formatCurrency, categoryParent } from "@/lib/utils"
import { CATEGORY_COLORS } from "@/lib/colors"
import { CATEGORY_TYPE_LABELS } from "@/bindings/category_type_groups"
import { useCategoryColorsContext } from "@/context/category_colors_context"
import { useCategoryMeta } from "@/context/category_names_context"

interface BudgetPieChartProps {
  rows: SpendingGridRow[]
  months: string[]
  granularity: Granularity
  groupBy: string
  accountNameMap: Record<string, string>
}

const PALETTE = Object.values(CATEGORY_COLORS)
const NEUTRAL = "#78716c"

export function BudgetPieChart({ rows, groupBy, accountNameMap }: BudgetPieChartProps) {
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

  const totals = new Map<string, number>()
  for (const row of rows) {
    const label = seriesLabel(row)
    let sum = 0
    for (const v of Object.values(row.periods)) {
      const n = parseFloat(v ?? "")
      if (Number.isFinite(n)) sum += Math.abs(n)
    }
    totals.set(label, (totals.get(label) ?? 0) + sum)
  }

  const data = Array.from(totals.entries())
    .sort(([, a], [, b]) => b - a)
    .map(([name, value]) => ({
      name,
      value: parseFloat(value.toFixed(2)),
    }))

  const colorByParent = (name: string) =>
    categoryColors[name] ?? CATEGORY_COLORS[name] ?? NEUTRAL
  const colors = data.map((d, i) => {
    if (groupBy === "leaf_category") return colorByParent(categoryParent(d.name))
    if (groupBy === "parent_category") return colorByParent(d.name)
    return PALETTE[i % PALETTE.length] ?? NEUTRAL
  })

  const totalSpending = data.reduce((s, d) => s + d.value, 0)

  return (
    <div className="rounded-lg border p-4">
      <h3 className="mb-2 text-sm font-medium text-muted-foreground">
        Spending Breakdown
      </h3>
      <InteractivePie
        data={data}
        colors={colors}
        label={`Total: ${formatCurrency(totalSpending.toFixed(2))}`}
        height={320}
        innerRadius={70}
        outerRadius={120}
      />
    </div>
  )
}
