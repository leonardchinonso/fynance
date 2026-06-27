import type { SpendingGridRow, Granularity } from "@/types"
import { StyledBarChart } from "@/components/charts"
import { formatPeriodKey, periodKeysFromRows } from "@/lib/utils"
import { CATEGORY_COLORS } from "@/lib/colors"
import { useCategoryColorsContext } from "@/context/category_colors_context"
import { useResolveCategoryName } from "@/context/category_names_context"

interface BudgetStackedBarProps {
  rows: SpendingGridRow[]
  months: string[]
  granularity: Granularity
}

export function BudgetStackedBar({ rows, granularity }: BudgetStackedBarProps) {
  const { categoryColors } = useCategoryColorsContext()
  const resolveName = useResolveCategoryName()
  const spendingRows = rows.filter(
    (r) => r.section === "Spending" || r.section === "Bills"
  )

  const categories = Array.from(
    new Set(spendingRows.map((r) => resolveName(r.category_id).split(":")[0].trim()))
  )

  // Rows are sparse per granularity period, so take the union of keys. A value
  // missing for a category in a period must read as 0, never NaN — NaN breaks the
  // Recharts stacked-bar baseline and makes every bar render full-height.
  const periodKeys = periodKeysFromRows(rows).filter((p) =>
    spendingRows.some((r) => r.periods[p] != null)
  )

  const data = periodKeys.map((p) => {
    const entry: Record<string, string | number> = {
      period: formatPeriodKey(p, granularity),
    }
    for (const cat of categories) {
      const catRows = spendingRows.filter(
        (r) => resolveName(r.category_id).split(":")[0].trim() === cat
      )
      let total = 0
      for (const row of catRows) {
        const n = parseFloat(row.periods[p] ?? "")
        if (Number.isFinite(n)) total += Math.abs(n)
      }
      entry[cat] = parseFloat(total.toFixed(2))
    }
    return entry
  })

  const colors = categories.map((c) => categoryColors[c] ?? CATEGORY_COLORS[c] ?? "#78716c")

  return (
    <div className="rounded-lg border p-4">
      <h3 className="mb-2 text-sm font-medium text-muted-foreground">
        Spending by Category Over Time
      </h3>
      <StyledBarChart
        data={data}
        index="period"
        categories={categories}
        colors={colors}
        stack
        height={340}
      />
    </div>
  )
}
