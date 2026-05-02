import type { SpendingGridRow, Granularity } from "@/types"
import type { RemoteData } from "@/lib/remote_data"
import { visitRemoteData } from "@/lib/remote_data"
import { BudgetChartsSkeleton } from "@/components/skeletons"
import { NonIdealState } from "@/components/non_ideal_state"
import { ReloadingOverlay } from "@/components/reloading_overlay"
import { BudgetStackedBar } from "./budget_stacked_bar"
import { BudgetLineChart } from "./budget_line_chart"
import { BudgetPieChart } from "./budget_pie_chart"

interface BudgetChartsProps {
  data: RemoteData<SpendingGridRow[]>
  months: string[]
  granularity: Granularity
}

export function BudgetCharts({ data, months, granularity }: BudgetChartsProps) {
  return visitRemoteData(data, {
    notLoaded: () => <BudgetChartsSkeleton />,
    failed: (error) => <NonIdealState title="Could not load charts" description={error} />,
    hasValue: (rows) => (
      <div className="relative">
        <BudgetChartsInternal rows={rows} months={months} granularity={granularity} />
        <ReloadingOverlay active={data.status === "reloading"} />
      </div>
    ),
  })
}

function BudgetChartsInternal({ rows, months, granularity }: { rows: SpendingGridRow[]; months: string[]; granularity: Granularity }) {
  return (
    <div className="space-y-4">
      <div className="grid gap-4 lg:grid-cols-2">
        <BudgetStackedBar rows={rows} months={months} granularity={granularity} />
        <BudgetPieChart rows={rows} />
      </div>
      <BudgetLineChart rows={rows} months={months} granularity={granularity} />
    </div>
  )
}
