import type { CategoryTotal } from "@/types"
import { InteractivePie } from "@/components/charts"
import { formatCurrency } from "@/lib/utils"
import { CATEGORY_COLORS } from "@/lib/colors"

interface TransactionPieChartProps {
  // Pre-aggregated totals from /api/transactions/by-category.
  // Caller should request with direction="outflow" so totals are positive.
  totals: CategoryTotal[]
}

export function TransactionPieChart({ totals }: TransactionPieChartProps) {
  // Roll leaf categories up to parent (see transaction_bar_chart for rationale).
  const byParent = new Map<string, number>()
  for (const row of totals) {
    const parent = row.category.split(":")[0]?.trim() ?? "Other"
    byParent.set(parent, (byParent.get(parent) ?? 0) + parseFloat(row.total))
  }

  const totalSpending = Array.from(byParent.values()).reduce((s, v) => s + v, 0)

  const data = Array.from(byParent.entries())
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
