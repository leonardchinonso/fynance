import { useState } from "react"
import { useUrlFilters } from "@/hooks/use_url_filters"
import { DateRangeSelector } from "@/components/date_range_selector"
import { ViewModeSwitcher } from "@/components/view_mode_switcher"
import { ExportButton } from "@/components/export_button"
import { TransactionTable } from "./transactions/transaction_table"
import { TransactionBarChart } from "./transactions/transaction_bar_chart"
import { TransactionPieChart } from "./transactions/transaction_pie_chart"
import { Table2, BarChart3, Search, Check, ChevronsUpDown } from "lucide-react"
import { Input } from "@/components/ui/input"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList,
} from "@/components/ui/command"
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover"
import { cn } from "@/lib/utils"
import { useTransactions, useTransactionCharts, useFilterOptions } from "@/hooks/data"

const VIEW_MODES = [
  { value: "table",  label: "Table",  icon: <Table2 className="h-4 w-4" /> },
  { value: "charts", label: "Charts", icon: <BarChart3 className="h-4 w-4" /> },
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

export function TransactionsPage() {
  const {
    start, end, view, setView, page, setPage,
    accounts: selectedAccounts, setAccounts,
    categories: selectedCategories, setCategories,
    profileId, search, setSearch, setFilter,
  } = useUrlFilters()

  const [pageSize, setPageSize] = useState(() => {
    try { return parseInt(localStorage.getItem("fynance-page-size") ?? "25", 10) || 25 }
    catch { return 25 }
  })

  const transactionsData = useTransactions(
    start, end, selectedAccounts, selectedCategories, search, page, pageSize, profileId,
  )
  const chartData = useTransactionCharts(
    start, end, selectedAccounts, selectedCategories, profileId,
  )
  const filterOptions = useFilterOptions(profileId)

  const availableAccounts =
    filterOptions.status === "succeeded" || filterOptions.status === "reloading"
      ? filterOptions.value.accounts.map(a => a.id)
      : []
  const accountNameMap =
    filterOptions.status === "succeeded" || filterOptions.status === "reloading"
      ? Object.fromEntries(filterOptions.value.accounts.map(a => [a.id, a.name]))
      : {}
  const availableCategories =
    filterOptions.status === "succeeded" || filterOptions.status === "reloading"
      ? filterOptions.value.categories
      : []

  const resetFilters = () => setFilter({
    accounts: undefined, categories: undefined, search: undefined,
    preset: "last-12-months", start: undefined, end: undefined, page: "1",
  })

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center gap-3">
        <DateRangeSelector />
        <div className="flex-1" />
        <ViewModeSwitcher modes={VIEW_MODES} value={view} onChange={setView} />
        <ExportButton />
      </div>

      <div className="flex flex-wrap items-center gap-2">
        <div className="relative">
          <Search className="absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
          <Input
            placeholder="Search transactions..."
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            className="h-8 w-[200px] pl-8 text-sm"
          />
        </div>
        <MultiSelect label="Accounts" options={availableAccounts} selected={selectedAccounts} onChange={setAccounts} displayFn={(id) => accountNameMap[id] ?? id} />
        <MultiSelect label="Categories" options={availableCategories} selected={selectedCategories} onChange={setCategories} />
        {(selectedAccounts.length > 0 || selectedCategories.length > 0 || search) && (
          <Button variant="ghost" size="sm" onClick={() => { setAccounts([]); setCategories([]); setSearch("") }}>
            Clear filters
          </Button>
        )}
      </div>

      {view === "table" || view === "table" ? (
        <TransactionTable
          data={transactionsData}
          page={page}
          pageSize={pageSize}
          onPageChange={setPage}
          onPageSizeChange={(s) => { setPageSize(s); setPage(1) }}
          accountNames={accountNameMap}
          onResetFilters={selectedAccounts.length > 0 || selectedCategories.length > 0 || search.length > 0 ? resetFilters : undefined}
        />
      ) : (
        <div className="grid gap-4 lg:grid-cols-2">
          <TransactionBarChart data={chartData} />
          <TransactionPieChart data={chartData} />
        </div>
      )}
    </div>
  )
}
