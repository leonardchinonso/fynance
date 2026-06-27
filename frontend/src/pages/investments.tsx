import { useSearchParams } from "react-router-dom"
import { useUrlFilters } from "@/hooks/use_url_filters"
import { ViewModeSwitcher } from "@/components/view_mode_switcher"
import { DateRangeSelector } from "@/components/date_range_selector"
import { useAccounts, useInvestments, useInvestmentsOverview } from "@/hooks/data"
import { History, Layers } from "lucide-react"
import { EventsHistory } from "./investments/events_history"
import { InvestmentsOverview } from "./investments/investments_overview"

const VIEW_MODES = [
  { value: "overview", label: "Overview", icon: <Layers className="h-4 w-4" /> },
  { value: "history",  label: "History",  icon: <History className="h-4 w-4" /> },
]

export function InvestmentsPage() {
  const { view, setView, profileId, start, end } = useUrlFilters()

  const [searchParams, setSearchParams] = useSearchParams()
  const accountId = searchParams.get("inv_account") ?? undefined
  const symbol = searchParams.get("inv_symbol") ?? undefined
  const eventType = searchParams.get("inv_type") ?? undefined

  // Overview is the default; "history" is the only non-default view value.
  const activeView = view === "history" ? "history" : "overview"

  const [accountsData] = useAccounts(profileId)
  const accounts = accountsData.status === "succeeded" || accountsData.status === "reloading"
    ? accountsData.value : []

  const [eventsData, reloadEvents] = useInvestments({ accountId, symbol, eventType })
  const overviewData = useInvestmentsOverview(start, end, profileId)

  function setFilters(next: { accountId?: string; symbol?: string; eventType?: string }) {
    setSearchParams((prev) => {
      const p = new URLSearchParams(prev)
      const apply = (key: string, value?: string) => {
        if (value) p.set(key, value)
        else p.delete(key)
      }
      apply("inv_account", next.accountId)
      apply("inv_symbol", next.symbol)
      apply("inv_type", next.eventType)
      return p
    })
  }

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center gap-3">
        <h1 className="text-lg font-semibold">Investments</h1>
        {activeView === "overview" && <DateRangeSelector />}
        <div className="flex-1" />
        <ViewModeSwitcher modes={VIEW_MODES} value={activeView} onChange={setView} />
      </div>

      {activeView === "history" && (
        <EventsHistory
          data={eventsData}
          accounts={accounts}
          reload={() => reloadEvents()}
          accountId={accountId}
          symbol={symbol}
          eventType={eventType}
          onFilterChange={setFilters}
        />
      )}
      {activeView === "overview" && (
        <InvestmentsOverview
          data={overviewData}
          rangeLabel={`${start} to ${end}`}
          start={start}
          end={end}
          hasProfile={profileId !== undefined}
        />
      )}
    </div>
  )
}
