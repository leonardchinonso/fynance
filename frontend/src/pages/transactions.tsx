import { useState, useEffect } from "react"
import type { CategoryTotal, Transaction, PaginatedResponse } from "@/types"
import { api } from "@/api/client"
import { useUrlFilters } from "@/hooks/use_url_filters"
import { DateRangeSelector } from "@/components/date_range_selector"
import { ViewModeSwitcher } from "@/components/view_mode_switcher"
import { ExportButton } from "@/components/export_button"
import { TableSkeleton, ChartSkeleton } from "@/components/skeletons"
import { EmptyState } from "@/components/empty_state"
import { Currency } from "@/components/currency"
import { TransactionTable } from "./transactions/transaction_table"
import { TransactionBarChart } from "./transactions/transaction_bar_chart"
import { TransactionPieChart } from "./transactions/transaction_pie_chart"
import { Table2, BarChart3, Search } from "lucide-react"
import { Input } from "@/components/ui/input"
import { Badge } from "@/components/ui/badge"
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command"
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover"
import { Button } from "@/components/ui/button"
import { Check, ChevronsUpDown } from "lucide-react"
import { cn } from "@/lib/utils"

const VIEW_MODES = [
  { value: "table", label: "Table", icon: <Table2 className="h-4 w-4" /> },
  { value: "charts", label: "Charts", icon: <BarChart3 className="h-4 w-4" /> },
]

function MultiSelect({
  label,
  options,
  selected,
  onChange,
  displayFn,
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
        {selected.length > 0 && (
          <Badge variant="secondary" className="ml-1">
            {selected.length}
          </Badge>
        )}
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
                  onSelect={() => {
                    onChange(
                      selected.includes(opt)
                        ? selected.filter((s) => s !== opt)
                        : [...selected, opt]
                    )
                  }}
                >
                  <Check
                    className={cn(
                      "mr-2 h-4 w-4",
                      selected.includes(opt) ? "opacity-100" : "opacity-0"
                    )}
                  />
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
    start,
    end,
    view,
    setView,
    page,
    setPage,
    accounts: selectedAccounts,
    setAccounts,
    categories: selectedCategories,
    setCategories,
    profileId,
    search,
    setSearch,
    setFilter,
  } = useUrlFilters()

  const hasFilters =
    selectedAccounts.length > 0 ||
    selectedCategories.length > 0 ||
    search.length > 0
  // Single setFilter call so all URL params update from the same prev state;
  // calling setAccounts/setCategories/setSearch/setPreset individually loses
  // writes because React batches state updates.
  const resetFilters = () => {
    setFilter({
      accounts: undefined,
      categories: undefined,
      search: undefined,
      preset: "last-12-months",
      start: undefined,
      end: undefined,
      page: "1",
    })
  }

  const [result, setResult] = useState<PaginatedResponse<Transaction> | null>(null)
  // Server-aggregated outflow totals per category for bar/pie charts.
  // Use /api/transactions/by-category instead of fetching raw rows; the
  // backend does the SUM so the frontend never has to ship thousands of
  // rows across the wire just to aggregate them client-side.
  const [chartTotals, setChartTotals] = useState<CategoryTotal[]>([])
  const [availableAccounts, setAvailableAccounts] = useState<string[]>([])
  const [accountNameMap, setAccountNameMap] = useState<Record<string, string>>({})
  const [availableCategories, setAvailableCategories] = useState<string[]>([])
  const [loading, setLoading] = useState(true)
  const [pageSize, setPageSize] = useState(() => {
    try {
      const v = localStorage.getItem("fynance-page-size")
      return v ? parseInt(v, 10) : 25
    } catch { return 25 }
  })

  // Serialize array deps to avoid infinite re-render loops
  const accountsKey = selectedAccounts.join(",")
  const categoriesKey = selectedCategories.join(",")

  // Fetch paginated data for table view
  useEffect(() => {
    setLoading(true)
    api
      .getTransactions({
        start,
        end,
        accounts: selectedAccounts.length > 0 ? selectedAccounts : undefined,
        categories:
          selectedCategories.length > 0 ? selectedCategories : undefined,
        search: search || undefined,
        page,
        limit: pageSize,
        profile_id: profileId,
      })
      .then((r) => {
        setResult(r)
        setLoading(false)
      })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [start, end, accountsKey, categoriesKey, search, page, pageSize, profileId])

  // Fetch server-aggregated category totals for chart views. Always filters
  // to outflows so charts show spending (not net positions). Free-text
  // `search` is intentionally not forwarded: the aggregation endpoint
  // doesn't support it and the result would be misleading if it did.
  useEffect(() => {
    api
      .getTransactionsByCategory({
        start,
        end,
        accounts: selectedAccounts.length > 0 ? selectedAccounts : undefined,
        categories:
          selectedCategories.length > 0 ? selectedCategories : undefined,
        profile_id: profileId,
        direction: "outflow",
      })
      .then(setChartTotals)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [start, end, accountsKey, categoriesKey, profileId])

  // Fetch filter options
  useEffect(() => {
    api.getAccounts(profileId).then((accs) => {
      setAvailableAccounts(accs.map((a) => a.id))
      const nameMap: Record<string, string> = {}
      for (const a of accs) nameMap[a.id] = a.name
      setAccountNameMap(nameMap)
    })
    api.getCategories().then(setAvailableCategories)
  }, [profileId])

  // Sum of all category outflow totals in the selected period. The backend
  // has already filtered to negative amounts and returned absolute values,
  // so this is simply the sum of the chart totals (negated to match the
  // previous signed-spending display).
  const totalSpending = -chartTotals.reduce(
    (sum, row) => sum + parseFloat(row.total),
    0
  )

  return (
    <div className="space-y-4">
      {/* Controls */}
      <div className="flex flex-wrap items-center gap-3">
        <DateRangeSelector />
        <div className="flex-1" />
        <ViewModeSwitcher modes={VIEW_MODES} value={view} onChange={setView} />
        <ExportButton />
      </div>

      {/* Filters */}
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
        <MultiSelect
          label="Accounts"
          options={availableAccounts}
          selected={selectedAccounts}
          onChange={setAccounts}
          displayFn={(id) => accountNameMap[id] ?? id}
        />
        <MultiSelect
          label="Categories"
          options={availableCategories}
          selected={selectedCategories}
          onChange={setCategories}
        />
        {(selectedAccounts.length > 0 || selectedCategories.length > 0 || search) && (
          <Button
            variant="ghost"
            size="sm"
            onClick={() => {
              setAccounts([])
              setCategories([])
              setSearch("")
            }}
          >
            Clear filters
          </Button>
        )}
      </div>

      {/* Summary - hidden while loading */}
      {!loading && (
        <div className="flex items-center gap-4 text-sm text-muted-foreground">
          <span>
            {result?.total ?? 0} transactions
          </span>
          <span>
            Total spending:{" "}
            <Currency
              amount={totalSpending.toFixed(2)}
              className="font-medium"
            />
          </span>
        </div>
      )}

      {/* Content */}
      {loading ? (
        view === "table" ? <TableSkeleton rows={25} cols={5} /> : <ChartSkeleton height={320} />
      ) : view === "table" && result ? (
        result.data.length === 0 ? (
          <EmptyState
            action={
              hasFilters
                ? { label: "Reset filters", onClick: resetFilters }
                : undefined
            }
          />
        ) : (
          <TransactionTable
            transactions={result.data}
            total={result.total}
            page={result.page}
            limit={result.limit}
            onPageChange={setPage}
            onLimitChange={(newLimit) => {
              setPageSize(newLimit)
              setPage(1)
            }}
            accountNames={accountNameMap}
          />
        )
      ) : (view === "charts" || view === "bar" || view === "pie") ? (
        chartTotals.length === 0 ? (
          <EmptyState
            action={
              hasFilters
                ? { label: "Reset filters", onClick: resetFilters }
                : undefined
            }
          />
        ) : (
          <div className="grid gap-4 lg:grid-cols-2">
            <TransactionBarChart totals={chartTotals} />
            <TransactionPieChart totals={chartTotals} />
          </div>
        )
      ) : null}
    </div>
  )
}
