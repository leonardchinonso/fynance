import type { Transaction } from "@/types"
import { DonutChart } from "@tremor/react"
import { formatCurrency } from "@/lib/utils"
import { ChartLegend } from "@/components/chart_legend"

const CHART_COLORS = [
  "blue", "orange", "green", "violet", "pink",
  "cyan", "yellow", "indigo", "teal", "red",
  "amber", "emerald",
]

interface TransactionPieChartProps {
  transactions: Transaction[]
}

export function TransactionPieChart({
  transactions,
}: TransactionPieChartProps) {
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
    }))

  const legendItems = data.map((d, i) => ({
    name: `${d.name} (${((d.value / totalSpending) * 100).toFixed(0)}%)`,
    color: CHART_COLORS[i % CHART_COLORS.length],
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
        colors={CHART_COLORS.slice(0, data.length)}
        valueFormatter={(v) => formatCurrency(v.toString())}
        className="h-72"
        showLabel
        label={`Total: ${formatCurrency(totalSpending.toFixed(2))}`}
      />
      <ChartLegend items={legendItems} className="mt-4 justify-center" />
    </div>
  )
}
