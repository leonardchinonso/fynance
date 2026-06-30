import { useSearchParams } from "react-router-dom"
import { useUrlFilters } from "@/hooks/use_url_filters"
import { useProfiles } from "@/context/profile_context"
import { DateRangeSelector } from "@/components/date_range_selector"
import { ViewModeSwitcher } from "@/components/view_mode_switcher"
import { PortfolioOverview } from "./portfolio/portfolio_overview"
import { AccountsGrid } from "./portfolio/accounts_grid"
import { InvestmentsDetail } from "./portfolio/investments_detail"
import { PortfolioHistory } from "./portfolio/portfolio_history"
import { LayoutDashboard, Grid3X3, LineChart } from "lucide-react"
import { usePortfolioSummary, usePortfolioAccounts, useCashSummary } from "@/hooks/data"

const VIEW_MODES = [
  { value: "overview", label: "Overview", icon: <LayoutDashboard className="h-4 w-4" /> },
  { value: "accounts", label: "Accounts", icon: <Grid3X3 className="h-4 w-4" /> },
  { value: "history",  label: "History",  icon: <LineChart className="h-4 w-4" /> },
]

export function PortfolioPage() {
  const {
    view, setView, profileId, start, end, granularity, hideSmall, assetClassSettings,
  } = useUrlFilters()
  const { profilesData } = useProfiles()

  const [searchParams, setSearchParams] = useSearchParams()
  const selectedAccountId = searchParams.get("account")
  const activeView = view === "table" ? "overview" : view

  // Demand-driven loads: each dataset fetches only when its consumer is visible.
  // Accounts is also consumed by the drill-down sheet (the page is its LCA), so
  // it stays loaded while a sheet is open regardless of the active tab. History
  // loads inside the History view itself (its sole consumer).
  const summaryData = usePortfolioSummary(
    start, end, granularity, profileId, activeView === "overview",
  )
  const cashSummaryData = useCashSummary(
    start, end, profileId, activeView === "overview",
  )
  const accountsData = usePortfolioAccounts(
    start, end, profileId, activeView === "accounts" || selectedAccountId !== null,
  )

  // Resolve the selected account (for the drill-down sheet) once accounts load.
  const selectedAccount =
    accountsData.status === "succeeded" || accountsData.status === "reloading"
      ? (accountsData.value.accounts.find(a => a.id === selectedAccountId) ?? null)
      : null

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center gap-3">
        <DateRangeSelector showGranularity={activeView === "history"} />
        <div className="flex-1" />
        <ViewModeSwitcher modes={VIEW_MODES} value={activeView} onChange={setView} />
      </div>

      {activeView === "overview" && (
        <PortfolioOverview
          data={summaryData}
          dateLabel={`${start} to ${end}`}
          start={start}
          end={end}
          cashSummary={cashSummaryData}
          assetClassSettings={assetClassSettings}
          hideSmall={hideSmall}
        />
      )}
      {activeView === "accounts" && (
        <AccountsGrid data={accountsData} profilesData={profilesData} onAccountClick={id => setSearchParams(p => { p.set("account", id); return p })} />
      )}
      {activeView === "history" && (
        <PortfolioHistory start={start} end={end} granularity={granularity} profileId={profileId} />
      )}

      <InvestmentsDetail
        accountId={selectedAccountId}
        account={selectedAccount}
        start={start}
        end={end}
        onClose={() => setSearchParams(p => { p.delete("account"); return p })}
      />
    </div>
  )
}
