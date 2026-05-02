import { useUrlFilters } from "@/hooks/use_url_filters"
import { DateRangeSelector } from "@/components/date_range_selector"
import { ViewModeSwitcher } from "@/components/view_mode_switcher"
import { ExportButton } from "@/components/export_button"
import { BudgetSpreadsheet } from "./budget/budget_spreadsheet"
import { BudgetCharts } from "./budget/budget_charts"
import { Grid3X3, BarChart3 } from "lucide-react"
import { getMonthsInRange } from "@/lib/utils"
import { useSpendingGrid } from "@/hooks/data"

const VIEW_MODES = [
  { value: "spreadsheet", label: "Spreadsheet", icon: <Grid3X3 className="h-4 w-4" /> },
  { value: "charts",      label: "Charts",      icon: <BarChart3 className="h-4 w-4" /> },
]

export function BudgetPage() {
  const { start, end, view, setView, granularity, profileId } = useUrlFilters()

  const [gridData, refreshGrid] = useSpendingGrid(start, end, granularity, profileId)
  const months = getMonthsInRange(start, end)

  const activeView =
    view === "stacked-bar" || view === "line" || view === "pie" || view === "charts"
      ? "charts"
      : "spreadsheet"

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center gap-3">
        <DateRangeSelector showGranularity />
        <div className="flex-1" />
        <ViewModeSwitcher modes={VIEW_MODES} value={activeView} onChange={setView} />
        <ExportButton />
      </div>

      {activeView === "spreadsheet" ? (
        <BudgetSpreadsheet data={gridData} months={months} granularity={granularity} onBudgetSaved={refreshGrid} />
      ) : (
        <BudgetCharts data={gridData} months={months} granularity={granularity} />
      )}
    </div>
  )
}
