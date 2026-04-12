import type { SpendingGridRow } from "@/types"
import { InteractivePie } from "@/components/charts"
import { formatCurrency } from "@/lib/utils"
import { CATEGORY_COLORS } from "@/lib/colors"

interface BudgetPieChartProps {
  rows: SpendingGridRow[]
}

export function BudgetPieChart({ rows }: BudgetPieChartProps) {
  const spendingRows = rows.filter(
    (r) => r.section === "Spending" || r.section === "Bills"
  )

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
  const colors = data.map((d) => CATEGORY_COLORS[d.name] ?? "#78716c")

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
