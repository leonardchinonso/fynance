import type { Transaction } from "@/types"
import { BarChart } from "@tremor/react"
import { CATEGORY_COLORS } from "@/lib/colors"
import { formatCurrency } from "@/lib/utils"

interface TransactionBarChartProps {
  transactions: Transaction[]
}

export function TransactionBarChart({
  transactions,
}: TransactionBarChartProps) {
  // Group spending by parent category
  const categorySpending = new Map<string, number>()
  for (const t of transactions) {
    const amt = parseFloat(t.amount)
    if (amt >= 0) continue // skip income
    const parent = t.category?.split(":")[0]?.trim() ?? "Other"
    categorySpending.set(
      parent,
      (categorySpending.get(parent) ?? 0) + Math.abs(amt)
    )
  }

  const data = Array.from(categorySpending.entries())
    .sort(([, a], [, b]) => b - a)
    .map(([category, amount]) => ({
      category,
      Spending: parseFloat(amount.toFixed(2)),
    }))

  const colors = data.map(
    (d) => CATEGORY_COLORS[d.category] ?? "#78716c"
  )

  return (
    <div className="rounded-lg border p-4">
      <h3 className="mb-4 text-sm font-medium text-muted-foreground">
        Spending by Category
      </h3>
      <BarChart
        data={data}
        index="category"
        categories={["Spending"]}
        colors={colors.length > 0 ? undefined : undefined}
        valueFormatter={(v) => formatCurrency(v.toString())}
        className="h-80"
        showLegend={false}
      />
    </div>
  )
}
