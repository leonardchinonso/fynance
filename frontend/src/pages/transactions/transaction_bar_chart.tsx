import type { CategoryTotal } from "@/types"
import { ColoredBarChart } from "@/components/charts"
import { CATEGORY_COLORS } from "@/lib/colors"

interface TransactionBarChartProps {
  // Pre-aggregated totals from /api/transactions/by-category.
  // Caller should request with direction="outflow" so totals are positive
  // absolute values rather than signed sums.
  totals: CategoryTotal[]
}

export function TransactionBarChart({ totals }: TransactionBarChartProps) {
  // Roll leaf categories (e.g. "Food: Groceries") up to their parent
  // ("Food") for display. The backend returns leaf rows because that's
  // the schema-level source of truth; the parent grouping is a display
  // concern.
  const byParent = new Map<string, number>()
  for (const row of totals) {
    const parent = row.category.split(":")[0]?.trim() ?? "Other"
    byParent.set(parent, (byParent.get(parent) ?? 0) + parseFloat(row.total))
  }

  const data = Array.from(byParent.entries())
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
