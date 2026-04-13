import { useState, useEffect } from "react"
import type { SpendingGridRow } from "@/types"
import { api } from "@/api/client"
import { useUrlFilters } from "@/hooks/use_url_filters"
import { DateRangeSelector } from "@/components/date_range_selector"
import { ViewModeSwitcher } from "@/components/view_mode_switcher"
import { ExportButton } from "@/components/export_button"
import { SpreadsheetSkeleton, BudgetChartsSkeleton } from "@/components/skeletons"
import { BudgetSpreadsheet } from "./budget/budget_spreadsheet"
import { BudgetCharts } from "./budget/budget_charts"
import { EmptyState } from "@/components/empty_state"
import { Grid3X3, BarChart3 } from "lucide-react"
import { getMonthsInRange } from "@/lib/utils"

const VIEW_MODES = [
  {
    value: "spreadsheet",
    label: "Spreadsheet",
    icon: <Grid3X3 className="h-4 w-4" />,
  },
  {
    value: "charts",
    label: "Charts",
    icon: <BarChart3 className="h-4 w-4" />,
  },
]

export function BudgetPage() {
  const {
    start,
    end,
    view,
    setView,
    granularity,
    profileId,
    accounts,
    categories,
    setFilter,
  } = useUrlFilters()

  const hasFilters = accounts.length > 0 || categories.length > 0
  // Single setFilter call so all URL params update from the same prev state.
  const resetFilters = () => {
    setFilter({
      accounts: undefined,
      categories: undefined,
      preset: "last-12-months",
      start: undefined,
      end: undefined,
    })
  }

  const [gridRows, setGridRows] = useState<SpendingGridRow[]>([])
  const [loading, setLoading] = useState(true)

  const months = getMonthsInRange(start, end)

  useEffect(() => {
    setLoading(true)
    api.getSpendingGrid(start, end, granularity, profileId).then((rows) => {
      setGridRows(rows)
      setLoading(false)
    })
  }, [start, end, granularity, profileId])

  // Map old view names to new ones
  const activeView =
    view === "stacked-bar" || view === "line" || view === "pie" || view === "charts"
      ? "charts"
      : "spreadsheet"

  // Show granularity on both spreadsheet and charts (stacked bar + line use it)
  const showGranularity = true

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center gap-3">
        <DateRangeSelector showGranularity={showGranularity} />
        <div className="flex-1" />
        <ViewModeSwitcher
          modes={VIEW_MODES}
          value={activeView}
          onChange={setView}
        />
        <ExportButton />
      </div>

      {loading ? (
        activeView === "spreadsheet" ? <SpreadsheetSkeleton /> : <BudgetChartsSkeleton />
      ) : gridRows.length === 0 ? (
        <EmptyState
          action={
            hasFilters
              ? { label: "Reset filters", onClick: resetFilters }
              : undefined
          }
        />
      ) : activeView === "spreadsheet" ? (
        <BudgetSpreadsheet rows={gridRows} months={months} granularity={granularity} />
      ) : activeView === "charts" ? (
        <BudgetCharts rows={gridRows} months={months} granularity={granularity} />
      ) : null}
    </div>
  )
}
