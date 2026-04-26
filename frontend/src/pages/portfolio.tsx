import { useState } from "react"
import { useUrlFilters } from "@/hooks/use_url_filters"
import { useProfiles } from "@/context/profile_context"
import { DateRangeSelector } from "@/components/date_range_selector"
import { ViewModeSwitcher } from "@/components/view_mode_switcher"
import { ExportButton } from "@/components/export_button"
import { PortfolioOverview } from "./portfolio/portfolio_overview"
import { AccountsGrid } from "./portfolio/accounts_grid"
import { HoldingsDetail } from "./portfolio/holdings_detail"
import { PortfolioCharts } from "./portfolio/portfolio_charts"
import { PortfolioHistory } from "./portfolio/portfolio_history"
import { LayoutDashboard, Grid3X3, PieChart, LineChart } from "lucide-react"
import { usePortfolioSummary, usePortfolioAccounts, usePortfolioHistoryData } from "@/hooks/data"

const VIEW_MODES = [
  { value: "overview",  label: "Overview",  icon: <LayoutDashboard className="h-4 w-4" /> },
  { value: "accounts",  label: "Accounts",  icon: <Grid3X3 className="h-4 w-4" /> },
  { value: "charts",    label: "Charts",    icon: <PieChart className="h-4 w-4" /> },
  { value: "history",   label: "History",   icon: <LineChart className="h-4 w-4" /> },
]

export function PortfolioPage() {
  const { view, setView, profileId, start, end, granularity } = useUrlFilters()
  const { profilesData } = useProfiles()

  const summaryData  = usePortfolioSummary(start, end, granularity, profileId)
  const accountsData = usePortfolioAccounts(start, end, profileId)
  const historyData  = usePortfolioHistoryData(start, end, granularity, profileId)

  const [selectedAccountId, setSelectedAccountId] = useState<string | null>(null)

  // Derive selected account name when accounts data is available
  const selectedAccountName =
    accountsData.status === "succeeded" || accountsData.status === "reloading"
      ? (accountsData.value.accounts.find(a => a.id === selectedAccountId)?.name ?? "")
      : ""

  const activeView = view === "table" ? "overview" : view

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center gap-3">
        <DateRangeSelector showGranularity={activeView === "history"} />
        <div className="flex-1" />
        <ViewModeSwitcher modes={VIEW_MODES} value={activeView} onChange={setView} />
        <ExportButton />
      </div>

      {activeView === "overview" && (
        <PortfolioOverview data={summaryData} dateLabel={`${start} to ${end}`} />
      )}
      {activeView === "accounts" && (
        <AccountsGrid data={accountsData} profilesData={profilesData} onAccountClick={setSelectedAccountId} />
      )}
      {activeView === "charts" && (
        <PortfolioCharts data={summaryData} />
      )}
      {activeView === "history" && (
        <PortfolioHistory data={historyData} granularity={granularity} />
      )}

      <HoldingsDetail
        accountId={selectedAccountId}
        accountName={selectedAccountName}
        onClose={() => setSelectedAccountId(null)}
      />
    </div>
  )
}
