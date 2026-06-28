import type { SpendingGridRow, Granularity } from "@/types"
import type { RemoteData } from "@/lib/remote_data"
import { visitRemoteData } from "@/lib/remote_data"
import { BudgetChartsSkeleton } from "@/components/skeletons"
import { AuthAwareError } from "@/components/auth_aware_error"
import { ReloadingOverlay } from "@/components/reloading_overlay"
import { BudgetStackedBar } from "./budget_stacked_bar"
import { BudgetLineChart } from "./budget_line_chart"
import { BudgetPieChart } from "./budget_pie_chart"

interface BudgetChartsProps {
  data: RemoteData<SpendingGridRow[]>
  months: string[]
  granularity: Granularity
  groupBy: string
  accountNameMap: Record<string, string>
}

export function BudgetCharts({ data, months, granularity, groupBy, accountNameMap }: BudgetChartsProps) {
  return visitRemoteData(data, {
    notLoaded: () => <BudgetChartsSkeleton />,
    failed: (error) => <AuthAwareError error={error} />,
    hasValue: (rows) => (
      <div className="relative">
        <BudgetChartsInternal
          rows={rows}
          months={months}
          granularity={granularity}
          groupBy={groupBy}
          accountNameMap={accountNameMap}
        />
        <ReloadingOverlay active={data.status === "reloading"} />
      </div>
    ),
  })
}

function BudgetChartsInternal({
  rows,
  months,
  granularity,
  groupBy,
  accountNameMap,
}: {
  rows: SpendingGridRow[]
  months: string[]
  granularity: Granularity
  groupBy: string
  accountNameMap: Record<string, string>
}) {
  return (
    <div className="space-y-4">
      <div className="grid gap-4 lg:grid-cols-2">
        <BudgetStackedBar
          rows={rows}
          months={months}
          granularity={granularity}
          groupBy={groupBy}
          accountNameMap={accountNameMap}
        />
        <BudgetPieChart
          rows={rows}
          months={months}
          granularity={granularity}
          groupBy={groupBy}
          accountNameMap={accountNameMap}
        />
      </div>
      <BudgetLineChart
        rows={rows}
        months={months}
        granularity={granularity}
        groupBy={groupBy}
        accountNameMap={accountNameMap}
      />
    </div>
  )
}
