import type { CategoryTotal } from "@/types"
import type { RemoteData } from "@/lib/remote_data"
import { visitRemoteData } from "@/lib/remote_data"
import { ChartSkeleton } from "@/components/skeletons"
import { NonIdealState } from "@/components/non_ideal_state"
import { ReloadingOverlay } from "@/components/reloading_overlay"
import { ColoredBarChart } from "@/components/charts"
import { CATEGORY_COLORS } from "@/lib/colors"
import { categoryParent } from "@/lib/utils"

export function TransactionBarChart({ data }: { data: RemoteData<CategoryTotal[]> }) {
  return visitRemoteData(data, {
    notLoaded: () => <ChartSkeleton height={320} />,
    failed: (error) => <NonIdealState title="Could not load chart" description={error} layout="horizontal" />,
    hasValue: (totals) => (
      <div className="relative">
        <TransactionBarChartInternal totals={totals} />
        <ReloadingOverlay active={data.status === "reloading"} />
      </div>
    ),
  })
}

function TransactionBarChartInternal({ totals }: { totals: CategoryTotal[] }) {
  const byParent = new Map<string, number>()
  for (const row of totals) {
    const parent = categoryParent(row.category)
    byParent.set(parent, (byParent.get(parent) ?? 0) + parseFloat(row.total))
  }

  const chartData = Array.from(byParent.entries())
    .sort(([, a], [, b]) => b - a)
    .map(([category, amount]) => ({ category, Spending: parseFloat(amount.toFixed(2)) }))

  const colors = chartData.map((d) => CATEGORY_COLORS[d.category] ?? "#78716c")

  return (
    <div className="rounded-lg border p-4">
      <h3 className="mb-2 text-sm font-medium text-muted-foreground">Spending by Category</h3>
      <ColoredBarChart data={chartData} index="category" valueKey="Spending" colors={colors} height={320} />
    </div>
  )
}
