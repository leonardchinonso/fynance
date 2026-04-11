import type { Transaction } from "@/types"
import { ColoredBarChart } from "@/components/charts"
import { CATEGORY_COLORS } from "@/lib/colors"

interface TransactionBarChartProps {
  transactions: Transaction[]
}

export function TransactionBarChart({ transactions }: TransactionBarChartProps) {
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

  const data = Array.from(categorySpending.entries())
    .sort(([, a], [, b]) => b - a)
    .map(([category, amount]) => ({
      category,
      Spending: parseFloat(amount.toFixed(2)),
    }))

  const colors = data.map((d) => CATEGORY_COLORS[d.category] ?? "#78716c")

  return (
    <div className="rounded-lg border p-4">
      <h3 className="mb-2 text-sm font-medium text-muted-foreground">
        Spending by Category
      </h3>
      <ColoredBarChart
        data={data}
        index="category"
        valueKey="Spending"
        colors={colors}
        height={320}
      />
    </div>
  )
}
