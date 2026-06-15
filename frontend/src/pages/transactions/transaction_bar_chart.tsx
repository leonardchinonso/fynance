import type { CategoryTotal } from "@/types"
import type { RemoteData } from "@/lib/remote_data"
import { visitRemoteData } from "@/lib/remote_data"
import { ChartSkeleton } from "@/components/skeletons"
import { AuthAwareError } from "@/components/auth_aware_error"
import { ReloadingOverlay } from "@/components/reloading_overlay"
import { ColoredBarChart } from "@/components/charts"
import { CATEGORY_COLORS } from "@/lib/colors"
import { categoryParent } from "@/lib/utils"
import { useResolveCategoryName } from "@/context/category_names_context"

export function TransactionBarChart({
  data,
  categoryColors = {},
}: {
  data: RemoteData<CategoryTotal[]>
  categoryColors?: Record<string, string>
}) {
  return visitRemoteData(data, {
    notLoaded: () => <ChartSkeleton height={320} />,
    failed: (error) => <AuthAwareError error={error} />,
    hasValue: (totals) => (
      <div className="relative">
        <TransactionBarChartInternal totals={totals} categoryColors={categoryColors} />
        <ReloadingOverlay active={data.status === "reloading"} />
      </div>
    ),
  })
}

function TransactionBarChartInternal({
  totals,
  categoryColors,
}: {
  totals: CategoryTotal[]
  categoryColors: Record<string, string>
}) {
  const resolveName = useResolveCategoryName()
  const byParent = new Map<string, number>()
  for (const row of totals) {
    const parent = categoryParent(resolveName(row.category_id))
    byParent.set(parent, (byParent.get(parent) ?? 0) + parseFloat(row.total))
  }

  const chartData = Array.from(byParent.entries())
    .sort(([, a], [, b]) => b - a)
    .map(([category, amount]) => ({ category, Spending: parseFloat(amount.toFixed(2)) }))

  const colors = chartData.map((d) => categoryColors[d.category] ?? CATEGORY_COLORS[d.category] ?? "#78716c")

  return (
    <div className="rounded-lg border p-4">
      <h3 className="mb-2 text-sm font-medium text-muted-foreground">Spending by Category</h3>
      <ColoredBarChart data={chartData} index="category" valueKey="Spending" colors={colors} height={320} />
    </div>
  )
}
