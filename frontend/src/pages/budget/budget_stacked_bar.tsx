import type { SpendingGridRow } from "@/types"
import { BarChart } from "@tremor/react"
import { formatCurrency, formatMonthShort } from "@/lib/utils"

interface BudgetStackedBarProps {
  rows: SpendingGridRow[]
  months: string[]
}

export function BudgetStackedBar({ rows, months }: BudgetStackedBarProps) {
  // Only spending categories (not income or transfers)
  const spendingRows = rows.filter(
    (r) => r.section === "Spending" || r.section === "Bills"
  )

  // Get unique parent categories
  const categories = Array.from(
    new Set(spendingRows.map((r) => r.category.split(":")[0].trim()))
  )

  // Build chart data: one entry per month
  const data = months.map((m) => {
    const entry: Record<string, string | number> = { month: formatMonthShort(m) }
    for (const cat of categories) {
      const catRows = spendingRows.filter(
        (r) => r.category.split(":")[0].trim() === cat
      )
      let total = 0
      for (const row of catRows) {
        total += Math.abs(parseFloat(row.months[m] ?? "0"))
      }
      entry[cat] = parseFloat(total.toFixed(2))
    }
    return entry
  })

  return (
    <div className="rounded-lg border p-4">
      <h3 className="mb-4 text-sm font-medium text-muted-foreground">
        Spending by Category Over Time
      </h3>
      <BarChart
        data={data}
        index="month"
        categories={categories}
        stack
        valueFormatter={(v) => formatCurrency(v.toString())}
        className="h-80"
      />
    </div>
  )
}
