import { useState, useEffect } from "react"
import type { PortfolioResponse, PortfolioHistoryRow, PortfolioSnapshot, CashFlowMonth, Holding } from "@/types"
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
      api.getPortfolioHistory(start, end),
      api.getAccountSnapshots(start, end),
      api.getCashFlow(start, end),
    ]).then(([p, h, snaps, cf]) => {
      setPortfolio(p)
      setHistory(h)
      setAccountSnapshots(snaps)
      setCashFlow(cf)
      // Fetch holdings for all investment + pension accounts
      const holdingAccounts = p.accounts.filter(
        (a) => a.type === "investment" || a.type === "pension"
      )
      Promise.all(holdingAccounts.map((a) => api.getHoldings(a.id))).then(
        (results) => setAllHoldings(results.flat())
      )
      setLoading(false)
    })
  }, [profileId, start, end])

  // Delta is start-of-range vs end-of-range net worth
  const startNetWorth =
    history.length >= 1 ? history[0].total_wealth : undefined
  const endNetWorth =
    history.length >= 1 ? history[history.length - 1].total_wealth : undefined

  // Investment metrics: compute from snapshots
  const investmentAccountIds = new Set(
    (portfolio?.accounts ?? [])
      .filter((a) => a.type === "investment")
      .map((a) => a.id)
  )

  // Start and end investment balances from snapshots
  const investStartBalances = new Map<string, number>()
  const investEndBalances = new Map<string, number>()
  for (const snap of accountSnapshots) {
    if (!investmentAccountIds.has(snap.account_id)) continue
    const month = snap.snapshot_date.substring(0, 7)
    if (!investStartBalances.has(snap.account_id) || month <= (start?.substring(0, 7) ?? "")) {
      investStartBalances.set(snap.account_id, parseFloat(snap.balance))
    }
    investEndBalances.set(snap.account_id, parseFloat(snap.balance))
  }
  const investStart = Array.from(investStartBalances.values()).reduce((s, v) => s + v, 0)
  const investEnd = Array.from(investEndBalances.values()).reduce((s, v) => s + v, 0)
  const investTotalGrowth = investEnd - investStart

  // New cash invested = sum of "Finance: Investment Transfer" spending
  // This is tracked as negative amounts in transactions
  const [newInvestments, setNewInvestments] = useState(0)
  useEffect(() => {
    if (!start || !end) return
    api.getTransactions({
      start, end, page: 1, limit: 10000, profile_id: profileId,
      categories: ["Finance: Investment Transfer"],
    }).then((r) => {
      const total = r.data
        .filter((t) => parseFloat(t.amount) < 0) // only outflows
        .reduce((s, t) => s + Math.abs(parseFloat(t.amount)), 0)
      setNewInvestments(total)
    })
  }, [start, end, profileId])

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
          cashFlow={cashFlow}
          holdings={allHoldings}
          investmentMetrics={{
            totalGrowth: investTotalGrowth,
            newCashInvested: newInvestments,
            marketGrowth: investTotalGrowth - newInvestments,
            startValue: investStart,
            endValue: investEnd,
          }}
        />
      ) : activeView === "accounts" ? (
        <AccountsGrid
          accounts={portfolio.accounts}
          onAccountClick={setSelectedAccountId}
          profiles={profiles}
          snapshots={accountSnapshots}
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
