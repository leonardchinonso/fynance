import type { SpendingGridRow, Granularity } from "@/types"
import { StyledLineChart } from "@/components/charts"
import {
  groupMonthsByGranularity,
  getMonthsForPeriod,
  formatPeriodKey,
} from "@/lib/utils"
import { CATEGORY_COLORS } from "@/lib/colors"
import { useCategoryColorsContext } from "@/context/category_colors_context"
import { useResolveCategoryName } from "@/context/category_names_context"

interface BudgetLineChartProps {
  rows: SpendingGridRow[]
  months: string[]
  granularity: Granularity
}

export function BudgetLineChart({ rows, months, granularity }: BudgetLineChartProps) {
  const { categoryColors } = useCategoryColorsContext()
  const resolveName = useResolveCategoryName()
  const spendingRows = rows.filter(
    (r) => r.section === "Spending" || r.section === "Bills"
  )

  const categories = Array.from(
    new Set(spendingRows.map((r) => resolveName(r.category_id).split(":")[0].trim()))
  ).slice(0, 8)

  const periods = groupMonthsByGranularity(months, granularity)

  // Only include periods that have data
  const periodsWithData = periods.filter((p) => {
    const periodMonths = getMonthsForPeriod(months, p, granularity)
    return spendingRows.some((r) =>
      periodMonths.some((m) => r.periods[m] !== null)
    )
  })

  const data = periodsWithData.map((p) => {
    const periodMonths = getMonthsForPeriod(months, p, granularity)
    const entry: Record<string, string | number> = {
      period: formatPeriodKey(p, granularity),
    }
    for (const cat of categories) {
      const catRows = spendingRows.filter(
        (r) => resolveName(r.category_id).split(":")[0].trim() === cat
      )
      let total = 0
      for (const row of catRows) {
        for (const m of periodMonths) {
          const val = row.periods[m]
          if (val !== null) total += Math.abs(parseFloat(val))
        }
      }
      entry[cat] = parseFloat(total.toFixed(2))
    }
    return entry
  })

  const colors = categories.map((c) => categoryColors[c] ?? CATEGORY_COLORS[c] ?? "#78716c")

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
