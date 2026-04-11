import { useState, useEffect } from "react"
import type { BudgetRow, SpendingGridRow } from "@/types"
import { api } from "@/api/client"
import { useUrlFilters } from "@/hooks/use_url_filters"
import { DateRangeSelector } from "@/components/date_range_selector"
import { ViewModeSwitcher } from "@/components/view_mode_switcher"
import { ExportButton } from "@/components/export_button"
import { LoadingSpinner } from "@/components/loading_spinner"
import { BudgetProgress } from "./budget/budget_progress"
import { BudgetSpreadsheet } from "./budget/budget_spreadsheet"
import { BudgetStackedBar } from "./budget/budget_stacked_bar"
import { BudgetLineChart } from "./budget/budget_line_chart"
import { BudgetPieChart } from "./budget/budget_pie_chart"
import { List, Grid3X3, BarChart3, LineChart, PieChart } from "lucide-react"
import { getMonthsInRange, getMonthFromDate } from "@/lib/utils"

const VIEW_MODES = [
  { value: "progress", label: "Progress", icon: <List className="h-4 w-4" /> },
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

  const [budgetRows, setBudgetRows] = useState<BudgetRow[]>([])
  const [gridRows, setGridRows] = useState<SpendingGridRow[]>([])
  const [loading, setLoading] = useState(true)

  const months = getMonthsInRange(start, end)
  const currentMonth = getMonthFromDate(end)

  // Fetch budget progress for current month
  useEffect(() => {
    setLoading(true)
    api.getBudget(currentMonth).then((rows) => {
      setBudgetRows(rows)
      setLoading(false)
    })
  }, [currentMonth])

  // Fetch spending grid for spreadsheet and chart views
  useEffect(() => {
    api.getSpendingGrid(start, end, granularity).then(setGridRows)
  }, [start, end, granularity])

  // Default to progress view
  const activeView = view === "table" ? "progress" : view

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
      ) : activeView === "progress" ? (
        <BudgetProgress rows={budgetRows} />
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
