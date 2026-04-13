import { useState, useEffect } from "react"
import type { PortfolioResponse, PortfolioHistoryRow, AccountSnapshot, CashFlowMonth, Holding } from "@/types"
import { api } from "@/api/client"
import { useUrlFilters } from "@/hooks/use_url_filters"
import { useProfiles } from "@/context/profile_context"
import { DateRangeSelector } from "@/components/date_range_selector"
import { ViewModeSwitcher } from "@/components/view_mode_switcher"
import { ExportButton } from "@/components/export_button"
import {
  PortfolioOverviewSkeleton,
  AccountsGridSkeleton,
  PortfolioChartsSkeleton,
  PortfolioHistorySkeleton,
} from "@/components/skeletons"
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
  const [accountBalances, setAccountBalances] = useState<AccountSnapshot[]>([])
  const [cashFlow, setCashFlow] = useState<CashFlowMonth[]>([])
  const [allHoldings, setAllHoldings] = useState<Holding[]>([])
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
      api.getPortfolioHistory(start, end, granularity, profileId),
      api.getAccountBalances(start, end, profileId),
      api.getCashFlow(start, end, granularity, profileId),
    ]).then(async ([p, h, snaps, cf]) => {
      setPortfolio(p)
      setHistory(h)
      setAccountBalances(snaps)
      setCashFlow(cf)
      // Fetch holdings for all investment + pension accounts
      const holdingAccounts = p.accounts.filter(
        (a) => a.type === "investment" || a.type === "pension"
      )
      const holdingResults = await Promise.all(
        holdingAccounts.map((a) => api.getHoldings(a.id))
      )
      setAllHoldings(holdingResults.flat())
      setLoading(false)
    })
  }, [profileId, start, end, granularity])

  // Delta is start-of-range vs end-of-range net worth
  const startNetWorth =
    history.length >= 1 ? history[0].total_wealth : undefined
  const endNetWorth =
    history.length >= 1 ? history[history.length - 1].total_wealth : undefined

  // Investment metrics now come from the backend as part of
  // `portfolio.investment_metrics` (computed server-side from holdings +
  // transaction transfers). The frontend no longer aggregates snapshots
  // or re-fetches Finance: Investment Transfer totals.

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
        activeView === "overview" ? <PortfolioOverviewSkeleton /> :
        activeView === "accounts" ? <AccountsGridSkeleton /> :
        activeView === "charts" ? <PortfolioChartsSkeleton /> :
        activeView === "history" ? <PortfolioHistorySkeleton /> :
        <PortfolioOverviewSkeleton />
      ) : activeView === "overview" ? (
        <PortfolioOverview
          portfolio={portfolio}
          startNetWorth={startNetWorth}
          endNetWorth={endNetWorth}
          dateLabel={`${start} to ${end}`}
          cashFlow={cashFlow}
          holdings={allHoldings}
          investmentMetrics={portfolio.investment_metrics}
        />
      ) : activeView === "accounts" ? (
        <AccountsGrid
          accounts={portfolio.accounts}
          onAccountClick={setSelectedAccountId}
          profiles={profiles}
          balances={accountBalances}
        />
      ) : activeView === "charts" ? (
        <PortfolioCharts portfolio={portfolio} holdings={allHoldings} />
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
