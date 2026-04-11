import type { Transaction } from "@/types"
import { InteractivePie } from "@/components/charts"
import { formatCurrency } from "@/lib/utils"
import { CATEGORY_COLORS } from "@/lib/colors"

interface TransactionPieChartProps {
  transactions: Transaction[]
}

export function TransactionPieChart({ transactions }: TransactionPieChartProps) {
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
    .map(([name, value]) => ({
      name,
      value: parseFloat(value.toFixed(2)),
    }))

  const colors = data.map((d) => CATEGORY_COLORS[d.name] ?? "#78716c")

  return (
    <div className="rounded-lg border p-4">
      <h3 className="mb-2 text-sm font-medium text-muted-foreground">
        Spending Distribution
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
