import type { SpendingGridRow, Granularity } from "@/types"
import { BudgetStackedBar } from "./budget_stacked_bar"
import { BudgetLineChart } from "./budget_line_chart"
import { BudgetPieChart } from "./budget_pie_chart"

interface BudgetChartsProps {
  rows: SpendingGridRow[]
  months: string[]
  granularity: Granularity
}

/**
 * Combined charts view showing stacked bar, line, and pie charts
 * on a single page instead of separate tabs.
 */
export function BudgetCharts({ rows, months, granularity }: BudgetChartsProps) {
  return (
    <div className="space-y-4">
      <div className="grid gap-4 lg:grid-cols-2">
        <BudgetStackedBar rows={rows} months={months} granularity={granularity} />
        <BudgetPieChart rows={rows} />
      </div>
      <BudgetLineChart rows={rows} months={months} granularity={granularity} />
    </div>
  )
}
