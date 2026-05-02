import type { CategoryTotal } from "@/types"
import type { RemoteData } from "@/lib/remote_data"
import { visitRemoteData } from "@/lib/remote_data"
import { ChartSkeleton } from "@/components/skeletons"
import { NonIdealState } from "@/components/non_ideal_state"
import { ReloadingOverlay } from "@/components/reloading_overlay"
import { InteractivePie } from "@/components/charts"
import { formatCurrency, categoryParent } from "@/lib/utils"
import { CATEGORY_COLORS } from "@/lib/colors"

export function TransactionPieChart({ data }: { data: RemoteData<CategoryTotal[]> }) {
  return visitRemoteData(data, {
    notLoaded: () => <ChartSkeleton height={320} />,
    failed: (error) => <NonIdealState title="Could not load chart" description={error} layout="horizontal" />,
    hasValue: (totals) => (
      <div className="relative">
        <TransactionPieChartInternal totals={totals} />
        <ReloadingOverlay active={data.status === "reloading"} />
      </div>
    ),
  })
}

function TransactionPieChartInternal({ totals }: { totals: CategoryTotal[] }) {
  const byParent = new Map<string, number>()
  for (const row of totals) {
    const parent = categoryParent(row.category)
    byParent.set(parent, (byParent.get(parent) ?? 0) + parseFloat(row.total))
  }

  const totalSpending = Array.from(byParent.values()).reduce((s, v) => s + v, 0)
  const pieData = Array.from(byParent.entries())
    .sort(([, a], [, b]) => b - a)
    .map(([name, value]) => ({ name, value: parseFloat(value.toFixed(2)) }))

  const colors = pieData.map((d) => CATEGORY_COLORS[d.name] ?? "#78716c")

  return (
    <div className="rounded-lg border p-4">
      <h3 className="mb-2 text-sm font-medium text-muted-foreground">Spending Distribution</h3>
      <InteractivePie
        data={pieData}
        colors={colors}
        label={`Total: ${formatCurrency(totalSpending.toFixed(2))}`}
        height={320}
        innerRadius={70}
        outerRadius={120}
      />
    </div>
  )
}
