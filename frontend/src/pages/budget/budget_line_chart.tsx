import type { SpendingGridRow } from "@/types"
import { LineChart } from "@tremor/react"
import { formatCurrency, formatMonthShort } from "@/lib/utils"

interface BudgetLineChartProps {
  rows: SpendingGridRow[]
  months: string[]
}

export function BudgetLineChart({ rows, months }: BudgetLineChartProps) {
  const spendingRows = rows.filter(
    (r) => r.section === "Spending" || r.section === "Bills"
  )

  const categories = Array.from(
    new Set(spendingRows.map((r) => r.category.split(":")[0].trim()))
  ).slice(0, 8) // Limit to 8 categories for readability

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
        Spending Trends
      </h3>
      <LineChart
        data={data}
        index="month"
        categories={categories}
        valueFormatter={(v) => formatCurrency(v.toString())}
        className="h-80"
      />
    </div>
  )
}
