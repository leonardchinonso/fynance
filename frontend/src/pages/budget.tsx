import { useState, useMemo, useEffect } from "react"
import { useSearchParams } from "react-router-dom"
import { useUrlFilters } from "@/hooks/use_url_filters"
import { usePageSizeParam } from "@/hooks/use_page_size"
import { DateRangeSelector } from "@/components/date_range_selector"
import { ViewModeSwitcher } from "@/components/view_mode_switcher"
import { MultiSelect } from "@/components/multi_select"
import { BudgetSpreadsheet } from "./budget/budget_spreadsheet"
import { BudgetCharts } from "./budget/budget_charts"
import { TransactionTable, BulkCategoryPicker } from "./transactions/transaction_table"
import { Grid3X3, Table2, BarChart3, Search, Trash2 } from "lucide-react"
import { Input } from "@/components/ui/input"
import { Button } from "@/components/ui/button"
import { ConfirmDialog } from "@/components/confirm_dialog"
import { showErrorToast } from "@/components/toast"
import {
  Select, SelectContent, SelectItem, SelectTrigger,
} from "@/components/ui/select"
import { getMonthsInRange } from "@/lib/utils"
import { useSpendingGrid, useTransactions, useFilterOptions, useCategoryOptions } from "@/hooks/data"
import { useCategoryColorsContext } from "@/context/category_colors_context"
import { api } from "@/api/client"
import type { CategoryType } from "@/bindings/CategoryType"
import { CATEGORY_TYPE_GROUPS, expandGroups, groupsForTypes } from "@/lib/category_types"

const VIEW_MODES = [
  { value: "overview", label: "Overview",     icon: <Grid3X3 className="h-4 w-4" /> },
  { value: "table",    label: "Transactions", icon: <Table2 className="h-4 w-4" /> },
  { value: "charts",   label: "Charts",       icon: <BarChart3 className="h-4 w-4" /> },
]

const GROUP_BY_OPTIONS: { value: string; label: string }[] = [
  { value: "parent_category", label: "Category" },
  { value: "leaf_category",   label: "Subcategory" },
  { value: "category_type",   label: "Category type" },
  { value: "account",         label: "Account" },
]

const UNCATEGORIZED = "__uncategorized__"

export function BudgetPage() {
  const [searchParams] = useSearchParams()
  // Bare /budget defaults to Overview (useUrlFilters' generic view default is
  // for other pages; we read it locally here).
  const view = searchParams.get("view") || "overview"

  const {
    start, end, granularity, profileId, page, setPage,
    accounts: selectedAccounts, setAccounts,
    categories: selectedCategories, setCategories,
    categoryTypes: selectedCategoryTypes, setCategoryTypes,
    groupBy, setGroupBy,
    search, setSearch, setView, setFilter,
    txSort, txDir, cycleTxSort,
  } = useUrlFilters()

  const [pageSize, setPageSize] = usePageSizeParam("limit", "page")

  const filterOptions = useFilterOptions(profileId)
  const availableAccounts =
    filterOptions.status === "succeeded" || filterOptions.status === "reloading"
      ? filterOptions.value.accounts.map((a) => a.id)
      : []
  const accountNameMap =
    filterOptions.status === "succeeded" || filterOptions.status === "reloading"
      ? Object.fromEntries(filterOptions.value.accounts.map((a) => [a.id, a.name]))
      : {}

  // Leaf categories with ids: powers the Category filter (we filter by id) and
  // the table's inline category editor.
  const categoryOptions = useCategoryOptions()
  const categoryNameById = useMemo(
    () => Object.fromEntries(categoryOptions.map((c) => [c.id, c.name])),
    [categoryOptions],
  )

  const { categoryColors, syncParents } = useCategoryColorsContext()
  const parentNames = useMemo(
    () => categoryOptions.map((c) => c.name.split(":")[0].trim()).filter((v, i, a) => a.indexOf(v) === i),
    [categoryOptions],
  )
  useEffect(() => { syncParents(parentNames) }, [parentNames.join(",")]) // eslint-disable-line react-hooks/exhaustive-deps

  const months = getMonthsInRange(start, end)

  // Charts default to Spending only; the user can select any mix of types via
  // the Types filter. The selection drives both the chart query and what the
  // filter shows as checked (Overview/Table treat an empty filter as "all").
  const chartTypes = selectedCategoryTypes.length > 0
    ? (selectedCategoryTypes as CategoryType[])
    : (["spending"] as CategoryType[])
  const typeFilterSelected = view === "charts" && selectedCategoryTypes.length === 0
    ? ["spending"]
    : groupsForTypes(selectedCategoryTypes)

  const sharedFilters = {
    accounts: selectedAccounts,
    categories: selectedCategories,
    categoryTypes: selectedCategoryTypes,
  }

  // Each view fetches only when active (enabled gate).
  const [gridData, refreshGrid] = useSpendingGrid(
    start, end, granularity, profileId, sharedFilters, view === "overview",
  )
  const [chartGrid] = useSpendingGrid(
    start, end, granularity, profileId,
    {
      accounts: selectedAccounts,
      categories: selectedCategories,
      categoryTypes: chartTypes,
      groupBy: groupBy as "leaf_category" | "parent_category" | "category_type" | "account",
    },
    view === "charts",
  )
  const transactionsData = useTransactions(
    start, end, selectedAccounts, selectedCategories, selectedCategoryTypes,
    search, page, pageSize, profileId, txSort, txDir, view === "table",
  )

  // Transaction multi-select lives here so the bulk-action bar can sit inline
  // with the filters (not in a new row that shifts the table on first select).
  const [selectedTxnIds, setSelectedTxnIds] = useState<Set<string>>(new Set())
  const [deletingIds, setDeletingIds] = useState<string[] | null>(null)
  const [deleteBusy, setDeleteBusy] = useState(false)
  const [deleteError, setDeleteError] = useState<string | null>(null)
  // Clear selection whenever the visible result set changes.
  const txKey = [
    start, end, selectedAccounts.join(","), selectedCategories.join(","),
    selectedCategoryTypes.join(","), search, page, pageSize, txSort ?? "", txDir, profileId ?? "",
  ].join("|")
  useEffect(() => { setSelectedTxnIds(new Set()) }, [txKey])

  async function bulkSetCategory(opt: { id: string; name: string }) {
    const ids = [...selectedTxnIds]
    setSelectedTxnIds(new Set())
    try { await api.bulkSetCategory(ids, opt.id) }
    catch (e) { showErrorToast(e instanceof Error ? e.message : String(e)) }
  }

  function requestDeleteTxns(ids: string[]) {
    setDeleteError(null)
    setDeletingIds(ids)
  }

  async function confirmDeleteTxns() {
    if (!deletingIds) return
    const ids = deletingIds
    setDeleteBusy(true)
    setDeleteError(null)
    try {
      if (ids.length === 1) await api.deleteTransaction(ids[0])
      else await api.bulkDeleteTransactions(ids)
      setSelectedTxnIds((prev) => { const n = new Set(prev); ids.forEach((id) => n.delete(id)); return n })
      setDeletingIds(null)
    } catch (e) { setDeleteError(e instanceof Error ? e.message : String(e)) }
    finally { setDeleteBusy(false) }
  }

  // A chart drill-down pins an explicit start/end, which overrides the preset.
  // Clearing has to drop those too, or the date range stays stuck on the drilled
  // period and the button looks like it did nothing.
  const hasExplicitRange = searchParams.has("start") || searchParams.has("end")
  const hasFilters =
    selectedAccounts.length > 0 || selectedCategories.length > 0 ||
    selectedCategoryTypes.length > 0 || search.length > 0 || hasExplicitRange
  const clearFilters = () => setFilter({
    accounts: undefined, categories: undefined, category_types: undefined, search: undefined,
    preset: "last-12-months", start: undefined, end: undefined, page: "1",
  })

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center gap-3">
        <DateRangeSelector showGranularity={view !== "table"} />
        <div className="flex-1" />
        <ViewModeSwitcher modes={VIEW_MODES} value={view} onChange={setView} />
      </div>

      <div className="flex flex-wrap items-center gap-2">
        <MultiSelect
          label="Accounts"
          options={availableAccounts}
          selected={selectedAccounts}
          onChange={setAccounts}
          displayFn={(id) => accountNameMap[id] ?? id}
        />
        <MultiSelect
          label="Categories"
          options={[UNCATEGORIZED, ...categoryOptions.map((c) => c.id)]}
          selected={selectedCategories}
          onChange={setCategories}
          displayFn={(v) => (v === UNCATEGORIZED ? "Uncategorized" : categoryNameById[v] ?? v)}
        />
        <MultiSelect
          label="Types"
          options={CATEGORY_TYPE_GROUPS.map((g) => g.key)}
          selected={typeFilterSelected}
          onChange={(keys) => setCategoryTypes(expandGroups(keys))}
          displayFn={(k) => CATEGORY_TYPE_GROUPS.find((g) => g.key === k)?.label ?? k}
        />
        {view === "charts" && (
          <div className="flex items-center gap-1.5">
            <span className="text-xs text-muted-foreground">Group by</span>
            <Select value={groupBy} onValueChange={(v) => { if (v) setGroupBy(v) }}>
              <SelectTrigger className="h-8 w-[140px] text-sm">
                <span>{GROUP_BY_OPTIONS.find((o) => o.value === groupBy)?.label ?? "Category"}</span>
              </SelectTrigger>
              <SelectContent>
                {GROUP_BY_OPTIONS.map((o) => (
                  <SelectItem key={o.value} value={o.value}>{o.label}</SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        )}
        {hasFilters && (
          <Button variant="ghost" size="sm" onClick={clearFilters}>Clear filters</Button>
        )}
        {view === "table" && selectedTxnIds.size > 0 && (
          <div className="flex items-center gap-2">
            <span className="text-sm text-muted-foreground">{selectedTxnIds.size} selected</span>
            <BulkCategoryPicker options={categoryOptions} onSelect={bulkSetCategory} />
            <Button
              variant="outline"
              size="sm"
              className="h-8 gap-1.5 text-destructive hover:text-destructive"
              onClick={() => requestDeleteTxns([...selectedTxnIds])}
            >
              <Trash2 className="h-3.5 w-3.5" /> Delete
            </Button>
            <Button variant="ghost" size="sm" className="h-8" onClick={() => setSelectedTxnIds(new Set())}>Clear</Button>
          </div>
        )}
        <div className="flex-1" />
        {view === "table" && (
          <div className="relative">
            <Search className="absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
            <Input
              placeholder="Search transactions..."
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              className="h-8 w-[200px] pl-8 text-sm"
            />
          </div>
        )}
      </div>

      {view === "overview" && (
        <BudgetSpreadsheet data={gridData} months={months} granularity={granularity} onBudgetSaved={refreshGrid} />
      )}
      {view === "table" && (
        <TransactionTable
          data={transactionsData}
          page={page}
          pageSize={pageSize}
          onPageChange={setPage}
          onPageSizeChange={setPageSize}
          accountNames={accountNameMap}
          categoryColors={categoryColors}
          categoryOptions={categoryOptions}
          sort={txSort}
          sortDir={txDir}
          onSort={cycleTxSort}
          onResetFilters={hasFilters ? clearFilters : undefined}
          selectedIds={selectedTxnIds}
          onSelectedChange={setSelectedTxnIds}
          onRequestDelete={requestDeleteTxns}
        />
      )}
      {view === "charts" && (
        <BudgetCharts data={chartGrid} months={months} granularity={granularity} groupBy={groupBy} accountNameMap={accountNameMap} />
      )}

      <ConfirmDialog
        open={deletingIds !== null}
        onOpenChange={(o) => { if (!o) setDeletingIds(null) }}
        title={`Delete ${deletingIds && deletingIds.length === 1 ? "transaction" : `${deletingIds?.length ?? 0} transactions`}?`}
        busy={deleteBusy}
        error={deleteError}
        onConfirm={confirmDeleteTxns}
      >
        This permanently deletes {deletingIds && deletingIds.length === 1 ? "this transaction" : "these transactions"}. This cannot be undone.
      </ConfirmDialog>
    </div>
  )
}
