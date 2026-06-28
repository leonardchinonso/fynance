import { useState } from "react"
import { useSearchParams } from "react-router-dom"
import { usePageSizeParam } from "@/hooks/use_page_size"
import { useUrlFilters } from "@/hooks/use_url_filters"
import { useProfiles } from "@/context/profile_context"
import { ViewModeSwitcher } from "@/components/view_mode_switcher"
import { DateRangeSelector } from "@/components/date_range_selector"
import { useAccounts, useInvestments, useInvestmentsOverview } from "@/hooks/data"
import { accountTypeToAssetClass } from "@/lib/account_type_utils"
import { History, Layers, Search, Check, ChevronsUpDown, Plus } from "lucide-react"
import { Input } from "@/components/ui/input"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList,
} from "@/components/ui/command"
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover"
import { cn } from "@/lib/utils"
import { EVENT_TYPES, EventDialog } from "./investments/event_dialog"
import { EventsHistory, type InvSortColumn, type SortDir } from "./investments/events_history"
import { InvestmentsOverview } from "./investments/investments_overview"

const VIEW_MODES = [
  { value: "overview", label: "Overview", icon: <Layers className="h-4 w-4" /> },
  { value: "history",  label: "History",  icon: <History className="h-4 w-4" /> },
]

function MultiSelect({
  label, options, selected, onChange, displayFn,
}: {
  label: string
  options: string[]
  selected: string[]
  onChange: (selected: string[]) => void
  displayFn?: (value: string) => string
}) {
  const [open, setOpen] = useState(false)
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger className="inline-flex shrink-0 items-center justify-center gap-1 rounded-md border bg-background px-3 py-1 text-sm font-medium shadow-xs hover:bg-accent hover:text-accent-foreground h-8">
        {label}
        {selected.length > 0 && <Badge variant="secondary" className="ml-1">{selected.length}</Badge>}
        <ChevronsUpDown className="ml-1 h-3 w-3 opacity-50" />
      </PopoverTrigger>
      <PopoverContent className="w-[250px] p-0" align="start">
        <Command>
          <CommandInput placeholder={`Search ${label.toLowerCase()}...`} />
          <CommandList>
            <CommandEmpty>No results.</CommandEmpty>
            <CommandGroup>
              {options.map((opt) => (
                <CommandItem
                  key={opt}
                  value={`${displayFn ? displayFn(opt) : opt} ${opt}`}
                  onSelect={() => onChange(
                    selected.includes(opt) ? selected.filter(s => s !== opt) : [...selected, opt]
                  )}
                >
                  <Check className={cn("mr-2 h-4 w-4", selected.includes(opt) ? "opacity-100" : "opacity-0")} />
                  {displayFn ? displayFn(opt) : opt}
                </CommandItem>
              ))}
            </CommandGroup>
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  )
}

function parseList(raw: string | null): string[] {
  return raw ? raw.split(",").filter(Boolean) : []
}

export function InvestmentsPage() {
  const { view, setView, profileId, start, end } = useUrlFilters()

  const [searchParams, setSearchParams] = useSearchParams()
  const selectedAccounts = parseList(searchParams.get("inv_accounts"))
  const selectedTypes = parseList(searchParams.get("inv_types"))
  const search = searchParams.get("inv_search") ?? ""
  const page = parseInt(searchParams.get("inv_page") || "1", 10)
  const sortRaw = searchParams.get("inv_sort")
  const sort: InvSortColumn =
    sortRaw === "symbol" || sortRaw === "quantity" || sortRaw === "price" ? sortRaw : "date"
  const sortDir: SortDir = searchParams.get("inv_dir") === "asc" ? "asc" : "desc"
  const [pageSize, setPageSize] = usePageSizeParam("inv_limit", "inv_page")

  // Overview is the default; "history" is the only non-default view value.
  const activeView = view === "history" ? "history" : "overview"

  const [adding, setAdding] = useState(false)

  const [accountsData] = useAccounts(profileId)
  const accounts = accountsData.status === "succeeded" || accountsData.status === "reloading"
    ? accountsData.value : []

  const { profilesData } = useProfiles()
  const profiles = profilesData.status === "succeeded" || profilesData.status === "reloading"
    ? profilesData.value : []
  const profileNameMap = Object.fromEntries(profiles.map((p) => [p.id, p.name]))

  const investmentAccounts = accounts.filter(
    (a) => accountTypeToAssetClass(a.type) === "Investments",
  )
  const accountNameMap = Object.fromEntries(accounts.map((a) => [a.id, a.name]))

  // Two accounts can share a name (e.g. "Trading 212 Invest" across profiles),
  // so tag each with its profile(s) to disambiguate in the filter and table.
  const accountLabel = (id: string): string => {
    const name = accountNameMap[id] ?? id
    const ids = accounts.find((x) => x.id === id)?.profile_ids ?? []
    if (ids.length === 0) return name
    const profs = ids.map((pid) => profileNameMap[pid] ?? pid).filter(Boolean)
    return profs.length > 0 ? `${name} (${profs.join(", ")})` : name
  }

  // Demand-driven: each tab fetches only when it is the active view.
  const [eventsData, reloadEvents] = useInvestments(activeView === "history")
  const overviewData = useInvestmentsOverview(start, end, profileId, selectedAccounts, activeView === "overview")

  function setParams(updates: Record<string, string | undefined>) {
    setSearchParams((prev) => {
      const p = new URLSearchParams(prev)
      for (const [key, value] of Object.entries(updates)) {
        if (value === undefined || value === "") p.delete(key)
        else p.set(key, value)
      }
      return p
    })
  }

  const setSelectedAccounts = (ids: string[]) =>
    setParams({ inv_accounts: ids.join(","), inv_page: "1" })
  const setSelectedTypes = (types: string[]) =>
    setParams({ inv_types: types.join(","), inv_page: "1" })
  const setSearch = (q: string) =>
    setParams({ inv_search: q || undefined, inv_page: "1" })
  const setPage = (p: number) => setParams({ inv_page: p.toString() })

  function cycleSort(col: InvSortColumn) {
    if (sort !== col) setParams({ inv_sort: col, inv_dir: "asc", inv_page: "1" })
    else if (sortDir === "asc") setParams({ inv_sort: col, inv_dir: "desc", inv_page: "1" })
    else setParams({ inv_sort: undefined, inv_dir: undefined, inv_page: "1" })
  }

  function clearFilters() {
    setParams({
      inv_accounts: undefined, inv_types: undefined, inv_search: undefined, inv_page: "1",
    })
  }

  const anyFilter =
    selectedAccounts.length > 0 || selectedTypes.length > 0 || search.length > 0

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center gap-3">
        <DateRangeSelector />
        <div className="flex-1" />
        <ViewModeSwitcher modes={VIEW_MODES} value={activeView} onChange={setView} />
      </div>

      <div className="flex flex-wrap items-center gap-2">
        <MultiSelect
          label="Accounts"
          options={investmentAccounts.map((a) => a.id)}
          selected={selectedAccounts}
          onChange={setSelectedAccounts}
          displayFn={accountLabel}
        />
        <MultiSelect
          label="Type"
          options={[...EVENT_TYPES]}
          selected={selectedTypes}
          onChange={setSelectedTypes}
          displayFn={(t) => t.charAt(0).toUpperCase() + t.slice(1)}
        />
        {anyFilter && (
          <Button variant="ghost" size="sm" onClick={clearFilters}>
            Clear filters
          </Button>
        )}
        <div className="flex-1" />
        {activeView === "history" && (
          <>
            <div className="relative">
              <Search className="absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
              <Input
                placeholder="Search events..."
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                className="h-8 w-[200px] pl-8 text-sm"
              />
            </div>
            <Button size="sm" className="gap-1.5" onClick={() => setAdding(true)} disabled={accounts.length === 0}>
              <Plus className="h-3.5 w-3.5" /> Add event
            </Button>
          </>
        )}
      </div>

      {activeView === "history" && (
        <EventsHistory
          data={eventsData}
          accounts={accounts}
          accountLabel={accountLabel}
          reload={() => reloadEvents()}
          start={start}
          end={end}
          selectedAccounts={selectedAccounts}
          selectedTypes={selectedTypes}
          search={search}
          page={page}
          onPageChange={setPage}
          pageSize={pageSize}
          onPageSizeChange={setPageSize}
          sort={sort}
          sortDir={sortDir}
          onSort={cycleSort}
          onResetFilters={anyFilter ? clearFilters : undefined}
        />
      )}
      {activeView === "overview" && (
        <InvestmentsOverview
          data={overviewData}
          rangeLabel={`${start} to ${end}`}
          start={start}
          end={end}
        />
      )}

      {adding && (
        <EventDialog
          event={null}
          accounts={accounts}
          onClose={() => setAdding(false)}
          onSaved={() => { setAdding(false); reloadEvents() }}
        />
      )}
    </div>
  )
}
