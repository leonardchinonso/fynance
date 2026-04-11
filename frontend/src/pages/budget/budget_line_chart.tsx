import type { SpendingGridRow } from "@/types"
import { StyledLineChart } from "@/components/charts"
import { formatMonthShort } from "@/lib/utils"
import { CATEGORY_COLORS } from "@/lib/colors"

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
  ).slice(0, 8)

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

  const colors = categories.map((c) => CATEGORY_COLORS[c] ?? "#78716c")

  return (
    <div className="rounded-lg border p-4">
      <h3 className="mb-2 text-sm font-medium text-muted-foreground">
        Spending Trends
      </h3>
      <StyledLineChart
        data={data}
        index="month"
        categories={categories}
        colors={colors}
        height={340}
        curved
      />
    </div>
  )
}
