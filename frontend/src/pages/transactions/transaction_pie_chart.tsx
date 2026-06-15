import type { CategoryTotal } from "@/types"
import type { RemoteData } from "@/lib/remote_data"
import { visitRemoteData } from "@/lib/remote_data"
import { ChartSkeleton } from "@/components/skeletons"
import { AuthAwareError } from "@/components/auth_aware_error"
import { ReloadingOverlay } from "@/components/reloading_overlay"
import { InteractivePie } from "@/components/charts"
import { formatCurrency, categoryParent } from "@/lib/utils"
import { CATEGORY_COLORS } from "@/lib/colors"
import { usePreferredCurrency } from "@/context/preferred_currency_context"
import { useResolveCategoryName } from "@/context/category_names_context"

export function TransactionPieChart({
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
        <TransactionPieChartInternal totals={totals} categoryColors={categoryColors} />
        <ReloadingOverlay active={data.status === "reloading"} />
      </div>
    ),
  })
}

function TransactionPieChartInternal({
  totals,
  categoryColors,
}: {
  totals: CategoryTotal[]
  categoryColors: Record<string, string>
}) {
  const preferredCurrency = usePreferredCurrency()
  const resolveName = useResolveCategoryName()
  const byParent = new Map<string, number>()
  for (const row of totals) {
    const parent = categoryParent(resolveName(row.category_id))
    byParent.set(parent, (byParent.get(parent) ?? 0) + parseFloat(row.total))
  }

  const totalSpending = Array.from(byParent.values()).reduce((s, v) => s + v, 0)
  const pieData = Array.from(byParent.entries())
    .sort(([, a], [, b]) => b - a)
    .map(([name, value]) => ({ name, value: parseFloat(value.toFixed(2)) }))

  const colors = pieData.map((d) => categoryColors[d.name] ?? CATEGORY_COLORS[d.name] ?? "#78716c")

  return (
    <div className="rounded-lg border p-4">
      <h3 className="mb-2 text-sm font-medium text-muted-foreground">Spending Distribution</h3>
      <InteractivePie
        data={pieData}
        colors={colors}
        label={`Total: ${formatCurrency(totalSpending.toFixed(2), preferredCurrency)}`}
        height={320}
        innerRadius={70}
        outerRadius={120}
      />
    </div>
  )
}
