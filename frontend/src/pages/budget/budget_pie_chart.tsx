import type { SpendingGridRow } from "@/types"
import { DonutChart } from "@tremor/react"
import { formatCurrency } from "@/lib/utils"

interface BudgetPieChartProps {
  rows: SpendingGridRow[]
}

export function BudgetPieChart({ rows }: BudgetPieChartProps) {
  const spendingRows = rows.filter(
    (r) => r.section === "Spending" || r.section === "Bills"
  )

  // Aggregate by parent category
  const categoryTotals = new Map<string, number>()
  for (const row of spendingRows) {
    const parent = row.category.split(":")[0].trim()
    const total = Math.abs(parseFloat(row.total ?? "0"))
    categoryTotals.set(parent, (categoryTotals.get(parent) ?? 0) + total)
  }

  const data = Array.from(categoryTotals.entries())
    .sort(([, a], [, b]) => b - a)
    .map(([name, value]) => ({
      name,
      value: parseFloat(value.toFixed(2)),
    }))

  const totalSpending = data.reduce((s, d) => s + d.value, 0)

  return (
    <div className="rounded-lg border p-4">
      <h3 className="mb-4 text-sm font-medium text-muted-foreground">
        Spending Breakdown
      </h3>
      <DonutChart
        data={data}
        category="value"
        index="name"
        valueFormatter={(v) => formatCurrency(v.toString())}
        className="h-80"
        showLabel
        label={`Total: ${formatCurrency(totalSpending.toFixed(2))}`}
      />
    </div>
  )
}
