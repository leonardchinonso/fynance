import type { Transaction } from "@/types"
import { DonutChart } from "@tremor/react"
import { formatCurrency } from "@/lib/utils"

interface TransactionPieChartProps {
  transactions: Transaction[]
}

export function TransactionPieChart({
  transactions,
}: TransactionPieChartProps) {
  // Group spending by parent category
  const categorySpending = new Map<string, number>()
  for (const t of transactions) {
    const amt = parseFloat(t.amount)
    if (amt >= 0) continue
    const parent = t.category?.split(":")[0]?.trim() ?? "Other"
    categorySpending.set(
      parent,
      (categorySpending.get(parent) ?? 0) + Math.abs(amt)
    )
  }

  const totalSpending = Array.from(categorySpending.values()).reduce(
    (s, v) => s + v,
    0
  )

  const data = Array.from(categorySpending.entries())
    .sort(([, a], [, b]) => b - a)
    .map(([name, amount]) => ({
      name,
      value: parseFloat(amount.toFixed(2)),
      percent: ((amount / totalSpending) * 100).toFixed(1),
    }))

  return (
    <div className="rounded-lg border p-4">
      <h3 className="mb-4 text-sm font-medium text-muted-foreground">
        Spending Distribution
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
