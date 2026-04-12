import type { SpendingGridRow, Granularity } from "@/types"
import { StyledBarChart } from "@/components/charts"
import {
  groupMonthsByGranularity,
  getMonthsForPeriod,
  formatPeriodKey,
} from "@/lib/utils"
import { CATEGORY_COLORS } from "@/lib/colors"

interface BudgetStackedBarProps {
  rows: SpendingGridRow[]
  months: string[]
  granularity: Granularity
}

export function BudgetStackedBar({ rows, months, granularity }: BudgetStackedBarProps) {
  const spendingRows = rows.filter(
    (r) => r.section === "Spending" || r.section === "Bills"
  )

  const categories = Array.from(
    new Set(spendingRows.map((r) => r.category.split(":")[0].trim()))
  )

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
        (r) => r.category.split(":")[0].trim() === cat
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

  const colors = categories.map((c) => CATEGORY_COLORS[c] ?? "#78716c")

  return (
    <div className="rounded-lg border p-4">
      <h3 className="mb-2 text-sm font-medium text-muted-foreground">
        Spending by Category Over Time
      </h3>
      <StyledBarChart
        data={data}
        index="period"
        categories={categories}
        colors={colors}
        stack
        height={340}
      />
    </div>
  )
}
