import type { SpendingGridRow, Granularity } from "@/types"
import type { CategoryType } from "@/bindings/CategoryType"
import { InteractivePie } from "@/components/charts"
import { formatCurrency, categoryParent } from "@/lib/utils"
import { CATEGORY_COLORS } from "@/lib/colors"
import { groupLabelForType, colorForGroupLabel } from "@/lib/category_types"
import { useCategoryColorsContext } from "@/context/category_colors_context"
import { useCategoryMeta } from "@/context/category_names_context"
import { useUrlFilters } from "@/hooks/use_url_filters"
import { useChartContextMenu, ChartContextMenu } from "@/components/charts/chart_context_menu"
import { categoryFilterForSeries } from "./chart_drill"

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
  const { resolve, parentName, childIdsOf } = useCategoryMeta()
  const { setFilter } = useUrlFilters()
  const { menu, open, close } = useChartContextMenu()

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
    if (groupBy === "category_type") return colorForGroupLabel(d.name)
    return PALETTE[i % PALETTE.length] ?? NEUTRAL
  })

  const totalSpending = data.reduce((s, d) => s + d.value, 0)

  const handleContextMenu = (
    e: { clientX: number; clientY: number; preventDefault: () => void },
    ctx: { index: number | null },
  ) => {
    if (ctx.index == null) return
    const name = data[ctx.index]?.name
    if (!name) return
    const catFilter = categoryFilterForSeries(rows, name, groupBy, seriesLabel, childIdsOf)
    if (!catFilter) return
    open(e, [{
      label: `View ${name} transactions`,
      onSelect: () => setFilter({ view: "table", page: "1", ...catFilter }),
    }])
  }

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
        onContextMenu={handleContextMenu}
      />
      <ChartContextMenu menu={menu} onClose={close} />
    </div>
  )
}
