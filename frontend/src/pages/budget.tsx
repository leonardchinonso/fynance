import { useState, useEffect } from "react"
import type { SpendingGridRow } from "@/types"
import { api } from "@/api/client"
import { useUrlFilters } from "@/hooks/use_url_filters"
import { DateRangeSelector } from "@/components/date_range_selector"
import { ViewModeSwitcher } from "@/components/view_mode_switcher"
import { ExportButton } from "@/components/export_button"
import { LoadingSpinner } from "@/components/loading_spinner"
import { BudgetSpreadsheet } from "./budget/budget_spreadsheet"
import { BudgetStackedBar } from "./budget/budget_stacked_bar"
import { BudgetLineChart } from "./budget/budget_line_chart"
import { BudgetPieChart } from "./budget/budget_pie_chart"
import { Grid3X3, BarChart3, LineChart, PieChart, Info } from "lucide-react"
import { getMonthsInRange } from "@/lib/utils"
import { Card, CardContent } from "@/components/ui/card"

const VIEW_MODES = [
  {
    value: "spreadsheet",
    label: "Spreadsheet",
    icon: <Grid3X3 className="h-4 w-4" />,
  },
  {
    value: "stacked-bar",
    label: "Stacked Bar",
    icon: <BarChart3 className="h-4 w-4" />,
  },
  { value: "line", label: "Line", icon: <LineChart className="h-4 w-4" /> },
  { value: "pie", label: "Pie", icon: <PieChart className="h-4 w-4" /> },
]

export function BudgetPage() {
  const { start, end, view, setView, granularity } = useUrlFilters()

  const [gridRows, setGridRows] = useState<SpendingGridRow[]>([])
  const [loading, setLoading] = useState(true)

  const months = getMonthsInRange(start, end)

  // Fetch spending grid for all views
  useEffect(() => {
    setLoading(true)
    api.getSpendingGrid(start, end, granularity).then((rows) => {
      setGridRows(rows)
      setLoading(false)
    })
  }, [start, end, granularity])

  // Default to spreadsheet view
  const activeView =
    view === "table" || view === "progress" ? "spreadsheet" : view

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center gap-3">
        <DateRangeSelector showGranularity />
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
        <BudgetSpreadsheet rows={gridRows} months={months} />
      ) : activeView === "stacked-bar" ? (
        <BudgetStackedBar rows={gridRows} months={months} />
      ) : activeView === "line" ? (
        <BudgetLineChart rows={gridRows} months={months} />
      ) : activeView === "pie" ? (
        <BudgetPieChart rows={gridRows} />
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
