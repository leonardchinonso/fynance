import { useState, useEffect } from "react"
import type { PortfolioResponse, PortfolioHistoryRow, PortfolioSnapshot } from "@/types"
import { api } from "@/api/client"
import { useUrlFilters } from "@/hooks/use_url_filters"
import { useProfiles } from "@/context/profile_context"
import { DateRangeSelector } from "@/components/date_range_selector"
import { ViewModeSwitcher } from "@/components/view_mode_switcher"
import { ExportButton } from "@/components/export_button"
import { LoadingSpinner } from "@/components/loading_spinner"
import { PortfolioOverview } from "./portfolio/portfolio_overview"
import { AccountsGrid } from "./portfolio/accounts_grid"
import { HoldingsDetail } from "./portfolio/holdings_detail"
import { PortfolioCharts } from "./portfolio/portfolio_charts"
import { PortfolioHistory } from "./portfolio/portfolio_history"
import { LayoutDashboard, Grid3X3, PieChart, LineChart } from "lucide-react"

const VIEW_MODES = [
  {
    value: "overview",
    label: "Overview",
    icon: <LayoutDashboard className="h-4 w-4" />,
  },
  {
    value: "accounts",
    label: "Accounts",
    icon: <Grid3X3 className="h-4 w-4" />,
  },
  {
    value: "charts",
    label: "Charts",
    icon: <PieChart className="h-4 w-4" />,
  },
  {
    value: "history",
    label: "History",
    icon: <LineChart className="h-4 w-4" />,
  },
]

export function PortfolioPage() {
  const { view, setView, profileId, start, end, granularity } = useUrlFilters()
  const { profiles } = useProfiles()

  const [portfolio, setPortfolio] = useState<PortfolioResponse | null>(null)
  const [history, setHistory] = useState<PortfolioHistoryRow[]>([])
  const [accountSnapshots, setAccountSnapshots] = useState<PortfolioSnapshot[]>([])
  const [loading, setLoading] = useState(true)

  // Holdings drill-down state
  const [selectedAccountId, setSelectedAccountId] = useState<string | null>(
    null
  )
  const selectedAccount = portfolio?.accounts.find(
    (a) => a.id === selectedAccountId
  )

  useEffect(() => {
    setLoading(true)
    Promise.all([
      api.getPortfolio(profileId),
      api.getPortfolioHistory(start, end),
      api.getAccountSnapshots(start, end),
    ]).then(([p, h, snaps]) => {
      setPortfolio(p)
      setHistory(h)
      setAccountSnapshots(snaps)
      setLoading(false)
    })
  }, [profileId, start, end])

  // Delta is start-of-range vs end-of-range net worth
  const startNetWorth =
    history.length >= 1 ? history[0].total_wealth : undefined
  const endNetWorth =
    history.length >= 1 ? history[history.length - 1].total_wealth : undefined

  // Default to overview view
  const activeView = view === "table" ? "overview" : view

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center gap-3">
        <DateRangeSelector showGranularity={activeView === "history"} />
        <div className="flex-1" />
        <ViewModeSwitcher
          modes={VIEW_MODES}
          value={activeView}
          onChange={setView}
        />
        <ExportButton />
      </div>

      {loading || !portfolio ? (
        <LoadingSpinner />
      ) : activeView === "overview" ? (
        <PortfolioOverview
          portfolio={portfolio}
          startNetWorth={startNetWorth}
          endNetWorth={endNetWorth}
          dateLabel={`${start} to ${end}`}
        />
      ) : activeView === "accounts" ? (
        <AccountsGrid
          accounts={portfolio.accounts}
          onAccountClick={setSelectedAccountId}
          profiles={profiles}
          snapshots={accountSnapshots}
        />
      ) : activeView === "charts" ? (
        <PortfolioCharts portfolio={portfolio} />
      ) : activeView === "history" ? (
        <PortfolioHistory history={history} granularity={granularity} />
      ) : null}

      {/* Holdings drill-down sheet */}
      <HoldingsDetail
        accountId={selectedAccountId}
        accountName={selectedAccount?.name ?? ""}
        onClose={() => setSelectedAccountId(null)}
      />
    </div>
  )
}
