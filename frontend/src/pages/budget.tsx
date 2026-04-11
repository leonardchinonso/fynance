import { useState, useEffect } from "react"
import type { SpendingGridRow } from "@/types"
import { api } from "@/api/client"
import { useUrlFilters } from "@/hooks/use_url_filters"
import { DateRangeSelector } from "@/components/date_range_selector"
import { ViewModeSwitcher } from "@/components/view_mode_switcher"
import { ExportButton } from "@/components/export_button"
import { LoadingSpinner } from "@/components/loading_spinner"
import { BudgetSpreadsheet } from "./budget/budget_spreadsheet"
import { BudgetCharts } from "./budget/budget_charts"
import { Grid3X3, BarChart3, Info } from "lucide-react"
import { getMonthsInRange } from "@/lib/utils"
import { Card, CardContent } from "@/components/ui/card"

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
  const { start, end, view, setView, granularity, profileId } = useUrlFilters()

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

  // Hide granularity on charts view (pie doesn't use it, and the charts handle it internally)
  const showGranularity = activeView === "spreadsheet"

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
        <LoadingSpinner />
      ) : gridRows.length === 0 ? (
        <EmptyState message="No spending data for the selected date range." />
      ) : activeView === "spreadsheet" ? (
        <BudgetSpreadsheet rows={gridRows} months={months} granularity={granularity} />
      ) : activeView === "charts" ? (
        <BudgetCharts rows={gridRows} months={months} granularity={granularity} />
      ) : null}
    </div>
  )
}

function EmptyState({ message }: { message: string }) {
  return (
    <Card className="py-12">
      <CardContent className="flex flex-col items-center gap-3 text-center">
        <Info className="h-8 w-8 text-muted-foreground" />
        <p className="text-sm text-muted-foreground max-w-md">{message}</p>
      </CardContent>
    </Card>
  )
}
