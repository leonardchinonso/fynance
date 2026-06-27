import { useMemo } from "react"
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
import { usePortfolioSummary, usePortfolioAccounts, usePortfolioHistoryData, useCategories } from "@/hooks/data"
import type { CategoryNode } from "@/bindings/CategoryNode"

function expandToLeafIds(selectedIds: string[], tree: CategoryNode[]): string[] {
  if (selectedIds.length === 0) return []
  const selected = new Set(selectedIds)
  const result = new Set<string>()
  for (const parent of tree) {
    if (selected.has(parent.id)) {
      for (const child of parent.children) result.add(child.id)
      if (parent.children.length === 0) result.add(parent.id)
    } else {
      for (const child of parent.children) {
        if (selected.has(child.id)) result.add(child.id)
      }
    }
  }
  // Preserve any selected IDs that didn't match the tree (e.g. tree not loaded
  // yet, or stale URL); the backend will simply find no matching transactions.
  for (const id of selectedIds) {
    if (!result.has(id)) result.add(id)
  }
  return Array.from(result)
}

const VIEW_MODES = [
  { value: "overview", label: "Overview", icon: <LayoutDashboard className="h-4 w-4" /> },
  { value: "accounts", label: "Accounts", icon: <Grid3X3 className="h-4 w-4" /> },
  { value: "history",  label: "History",  icon: <LineChart className="h-4 w-4" /> },
]

export function PortfolioPage() {
  const {
    view, setView, profileId, start, end, granularity, hideSmall, assetClassSettings,
    excludedCategories, setExcludedCategories,
  } = useUrlFilters()
  const { profilesData } = useProfiles()

  const [categoriesData] = useCategories()
  const categoryTree = categoriesData.status === "succeeded" || categoriesData.status === "reloading"
    ? categoriesData.value : []

  const excludeLeafIds = useMemo(
    () => expandToLeafIds(excludedCategories, categoryTree),
    [excludedCategories, categoryTree],
  )

  const summaryData  = usePortfolioSummary(start, end, granularity, profileId, excludeLeafIds)
  const accountsData = usePortfolioAccounts(start, end, profileId)
  const historyData  = usePortfolioHistoryData(start, end, granularity, profileId)

  const [searchParams, setSearchParams] = useSearchParams()
  const selectedAccountId = searchParams.get("account")

  // Resolve the selected account (for the drill-down sheet) once accounts load.
  const selectedAccount =
    accountsData.status === "succeeded" || accountsData.status === "reloading"
      ? (accountsData.value.accounts.find(a => a.id === selectedAccountId) ?? null)
      : null

  const activeView = view === "table" ? "overview" : view

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
          assetClassSettings={assetClassSettings}
          hideSmall={hideSmall}
          categoryTree={categoryTree}
          excludedCategories={excludedCategories}
          setExcludedCategories={setExcludedCategories}
        />
      )}
      {activeView === "accounts" && (
        <AccountsGrid data={accountsData} profilesData={profilesData} onAccountClick={id => setSearchParams(p => { p.set("account", id); return p })} />
      )}
      {activeView === "history" && (
        <PortfolioHistory data={historyData} granularity={granularity} />
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
