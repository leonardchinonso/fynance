import { useEffect, useMemo, useState } from "react"
import { api } from "@/api/client"
import type { Account } from "@/types"
import type { InvestmentEvent } from "@/bindings/InvestmentEvent"
import type { RemoteData } from "@/lib/remote_data"
import { visitRemoteData } from "@/lib/remote_data"
import { formatDate, cn } from "@/lib/utils"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { EmptyState } from "@/components/empty_state"
import { AuthAwareError } from "@/components/auth_aware_error"
import { ReloadingOverlay } from "@/components/reloading_overlay"
import { TableSkeleton } from "@/components/skeletons"
import {
  Table, TableBody, TableCell, TableHead, TableHeader, TableRow,
} from "@/components/ui/table"
import {
  Select, SelectContent, SelectItem, SelectTrigger,
} from "@/components/ui/select"
import {
  Popover, PopoverContent, PopoverTrigger,
} from "@/components/ui/popover"
import {
  Pencil, Trash2, TrendingUp,
  ChevronLeft, ChevronRight, Settings2, Check, ArrowUp, ArrowDown, ArrowUpDown,
} from "lucide-react"
import { ConfirmDialog } from "@/components/confirm_dialog"
import { SourceChips, type SourceDocMeta } from "@/components/source_chips"
import { MoneyDisplay } from "@/components/currency"
import { colorForSymbol, EVENT_TYPE_COLORS } from "@/lib/colors"
import { usePreferredCurrency, useCurrenciesFromContext } from "@/context/preferred_currency_context"
import { EventDialog } from "./event_dialog"

const PAGE_SIZE_OPTIONS = [10, 25, 50, 100]
const COLUMNS_KEY = "fynance-inv-columns"

export type InvSortColumn = "date" | "symbol" | "quantity" | "price"
export type SortDir = "asc" | "desc"

interface Column {
  id: string
  label: string
  defaultVisible: boolean
}

const ALL_COLUMNS: Column[] = [
  { id: "date", label: "Date", defaultVisible: true },
  { id: "account", label: "Account", defaultVisible: true },
  { id: "symbol", label: "Symbol", defaultVisible: true },
  { id: "type", label: "Type", defaultVisible: true },
  { id: "quantity", label: "Quantity", defaultVisible: true },
  { id: "price", label: "Price/share", defaultVisible: true },
  { id: "fee", label: "Fee", defaultVisible: true },
  { id: "currency", label: "Currency", defaultVisible: true },
  { id: "source", label: "Source", defaultVisible: false },
  { id: "notes", label: "Notes", defaultVisible: false },
]

function getStoredColumns(): Set<string> {
  try {
    const v = localStorage.getItem(COLUMNS_KEY)
    if (v) return new Set(JSON.parse(v))
  } catch { /* ignore */ }
  return new Set(ALL_COLUMNS.filter((c) => c.defaultVisible).map((c) => c.id))
}

interface Props {
  data: RemoteData<InvestmentEvent[]>
  accounts: Account[]
  /** Renders an account id as "Name (Profile)" for display + search. */
  accountLabel: (id: string) => string
  reload: () => void
  /** Date range (YYYY-MM-DD); rows outside are filtered out. */
  start: string
  end: string
  /** Selected account ids (empty = all). */
  selectedAccounts: string[]
  /** Selected event types (empty = all). */
  selectedTypes: string[]
  /** Free-text search across symbol, notes and account name. */
  search: string
  page: number
  onPageChange: (page: number) => void
  pageSize: number
  onPageSizeChange: (size: number) => void
  sort: InvSortColumn
  sortDir: SortDir
  onSort: (col: InvSortColumn) => void
  /** Clears all client-side filters (accounts/types/search). */
  onResetFilters?: () => void
}

export function EventsHistory({
  data, accounts, accountLabel, reload, start, end,
  selectedAccounts, selectedTypes, search,
  page, onPageChange, pageSize, onPageSizeChange, sort, sortDir, onSort, onResetFilters,
}: Props) {
  const [editing, setEditing] = useState<InvestmentEvent | null>(null)
  const [deleting, setDeleting] = useState<InvestmentEvent | null>(null)

  async function handleDeleteConfirm() {
    if (!deleting) return
    try {
      await api.deleteInvestment(deleting.id)
      setDeleting(null)
      reload()
    } catch (err) {
      alert(err instanceof Error ? err.message : String(err))
    }
  }

  const filtersActive =
    selectedAccounts.length > 0 || selectedTypes.length > 0 || search.trim().length > 0

  return (
    <div className="space-y-4">
      {visitRemoteData(data, {
        notLoaded: () => <TableSkeleton rows={pageSize} cols={8} />,
        failed: (error) => <AuthAwareError error={error} onRetry={reload} />,
        hasValue: (events) => (
          <div className="relative">
            <EventsTable
              events={events}
              accountLabel={accountLabel}
              start={start}
              end={end}
              selectedAccounts={selectedAccounts}
              selectedTypes={selectedTypes}
              search={search}
              page={page}
              onPageChange={onPageChange}
              pageSize={pageSize}
              onPageSizeChange={onPageSizeChange}
              sort={sort}
              sortDir={sortDir}
              onSort={onSort}
              onEdit={setEditing}
              onDelete={setDeleting}
              onResetFilters={filtersActive ? onResetFilters : undefined}
            />
            <ReloadingOverlay active={data.status === "reloading"} />
          </div>
        ),
      })}

      {editing && (
        <EventDialog
          event={editing}
          accounts={accounts}
          onClose={() => setEditing(null)}
          onSaved={() => { setEditing(null); reload() }}
        />
      )}

      <ConfirmDialog
        open={!!deleting}
        onOpenChange={(open) => { if (!open) setDeleting(null) }}
        title="Delete investment event?"
        onConfirm={handleDeleteConfirm}
      >
        This permanently removes the <strong>{deleting?.event_type}</strong> event for{" "}
        <strong>{deleting?.symbol}</strong> on {deleting?.date.slice(0, 10)}.
      </ConfirmDialog>
    </div>
  )
}

function EventsTable({
  events, accountLabel, start, end, selectedAccounts, selectedTypes, search,
  page, onPageChange, pageSize, onPageSizeChange, sort, sortDir, onSort,
  onEdit, onDelete, onResetFilters,
}: {
  events: InvestmentEvent[]
  accountLabel: (id: string) => string
  start: string
  end: string
  selectedAccounts: string[]
  selectedTypes: string[]
  search: string
  page: number
  onPageChange: (page: number) => void
  pageSize: number
  onPageSizeChange: (size: number) => void
  sort: InvSortColumn
  sortDir: SortDir
  onSort: (col: InvSortColumn) => void
  onEdit: (e: InvestmentEvent) => void
  onDelete: (e: InvestmentEvent) => void
  onResetFilters?: () => void
}) {
  const [visibleColumns, setVisibleColumns] = useState<Set<string>>(getStoredColumns)
  const [docsMap, setDocsMap] = useState<Map<string, SourceDocMeta>>(new Map())

  useEffect(() => {
    let cancelled = false
    api
      .listDocuments()
      .then((docs) => {
        if (cancelled) return
        setDocsMap(new Map(docs.map((d) => [d.id, { filename: d.filename, uploaded_at: d.uploaded_at }])))
      })
      .catch(() => { /* documents are optional context; ignore */ })
    return () => { cancelled = true }
  }, [])

  function toggleColumn(colId: string) {
    setVisibleColumns((prev) => {
      const next = new Set(prev)
      if (next.has(colId)) next.delete(colId)
      else next.add(colId)
      localStorage.setItem(COLUMNS_KEY, JSON.stringify(Array.from(next)))
      return next
    })
  }
  const isVisible = (colId: string) => visibleColumns.has(colId)

  const accountSet = useMemo(() => new Set(selectedAccounts), [selectedAccounts])
  const typeSet = useMemo(() => new Set(selectedTypes), [selectedTypes])
  const needle = search.trim().toLowerCase()

  const preferredCurrency = usePreferredCurrency()
  const currencies = useCurrenciesFromContext()
  const fxRates = useMemo(
    () => Object.fromEntries(currencies.map(c => [c.code, c.fx_rate])),
    [currencies],
  )

  const filtered = useMemo(() => {
    const startDay = start.slice(0, 10)
    const endDay = end.slice(0, 10)
    return events.filter((e) => {
      const day = e.date.slice(0, 10)
      if (day < startDay || day > endDay) return false
      if (accountSet.size > 0 && !accountSet.has(e.account_id)) return false
      if (typeSet.size > 0 && !typeSet.has(e.event_type)) return false
      if (needle) {
        const haystack = [
          e.symbol,
          e.notes ?? "",
          accountLabel(e.account_id),
        ].join(" ").toLowerCase()
        if (!haystack.includes(needle)) return false
      }
      return true
    })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [events, start, end, accountSet, typeSet, needle, accountLabel])

  const sorted = useMemo(() => {
    const dir = sortDir === "asc" ? 1 : -1
    const arr = [...filtered]
    arr.sort((a, b) => {
      let cmp = 0
      switch (sort) {
        case "date":
          cmp = a.date.localeCompare(b.date)
          break
        case "symbol":
          cmp = a.symbol.localeCompare(b.symbol)
          break
        case "quantity":
          cmp = num(a.quantity) - num(b.quantity)
          break
        case "price":
          cmp = num(a.price_per_share) - num(b.price_per_share)
          break
      }
      if (cmp === 0) cmp = a.date.localeCompare(b.date)
      return cmp * dir
    })
    return arr
  }, [filtered, sort, sortDir])

  const total = sorted.length
  const totalPages = Math.max(1, Math.ceil(total / pageSize))
  const safePage = Math.min(page, totalPages)
  const pageRows = useMemo(
    () => sorted.slice((safePage - 1) * pageSize, safePage * pageSize),
    [sorted, safePage, pageSize],
  )

  if (total === 0) {
    return (
      <EmptyState
        icon={<TrendingUp className="h-8 w-8" />}
        title="No investment events"
        message="Add a buy, sell, or vest event, or adjust your filters to see more."
        action={onResetFilters ? { label: "Reset filters", onClick: onResetFilters } : undefined}
      />
    )
  }

  return (
    <div>
      <Table>
        <TableHeader>
          <TableRow>
            {isVisible("date") && (
              <SortableHeader label="Date" column="date" activeColumn={sort} direction={sortDir} onClick={onSort} />
            )}
            {isVisible("account") && <TableHead>Account</TableHead>}
            {isVisible("symbol") && (
              <SortableHeader label="Symbol" column="symbol" activeColumn={sort} direction={sortDir} onClick={onSort} />
            )}
            {isVisible("type") && <TableHead>Type</TableHead>}
            {isVisible("quantity") && (
              <SortableHeader label="Quantity" column="quantity" activeColumn={sort} direction={sortDir} onClick={onSort} align="right" className="text-right" />
            )}
            {isVisible("price") && (
              <SortableHeader label="Price/share" column="price" activeColumn={sort} direction={sortDir} onClick={onSort} align="right" className="text-right" />
            )}
            {isVisible("fee") && <TableHead className="text-right">Fee</TableHead>}
            {isVisible("currency") && <TableHead>Currency</TableHead>}
            {isVisible("source") && <TableHead>Source</TableHead>}
            {isVisible("notes") && <TableHead>Notes</TableHead>}
            <TableHead className="w-8">
              <ColumnSettings columns={ALL_COLUMNS} visible={visibleColumns} onToggle={toggleColumn} />
            </TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {pageRows.map((e) => (
            <TableRow key={e.id} className="group">
              {isVisible("date") && (
                <TableCell className="whitespace-nowrap tabular-nums">{formatDate(e.date)}</TableCell>
              )}
              {isVisible("account") && (
                <TableCell className="text-sm text-muted-foreground">{accountLabel(e.account_id)}</TableCell>
              )}
              {isVisible("symbol") && (
                <TableCell className="font-medium">
                  <span className="inline-flex items-center gap-1.5">
                    <span
                      className="h-2 w-2 shrink-0 rounded-full"
                      style={{ backgroundColor: colorForSymbol(e.symbol) }}
                    />
                    {e.symbol}
                  </span>
                </TableCell>
              )}
              {isVisible("type") && (
                <TableCell>
                  <Badge
                    variant="secondary"
                    className="capitalize font-normal border"
                    style={{
                      backgroundColor: `${EVENT_TYPE_COLORS[e.event_type]}1f`,
                      borderColor: `${EVENT_TYPE_COLORS[e.event_type]}59`,
                      color: EVENT_TYPE_COLORS[e.event_type],
                    }}
                  >
                    {e.event_type}
                  </Badge>
                </TableCell>
              )}
              {isVisible("quantity") && (
                <TableCell className="text-right tabular-nums">{fmtQty(e.quantity)}</TableCell>
              )}
              {isVisible("price") && (
                <TableCell className="text-right tabular-nums">
                  <MoneyDisplay
                    amount={e.price_per_share}
                    currency={e.currency}
                    colorize={false}
                    preferredCurrency={preferredCurrency}
                    fxRate={fxRates[e.currency]}
                  />
                </TableCell>
              )}
              {isVisible("fee") && (
                <TableCell className="text-right tabular-nums text-muted-foreground">
                  {e.fee ? (
                    <MoneyDisplay
                      amount={e.fee}
                      currency={e.fee_currency ?? e.currency}
                      colorize={false}
                      preferredCurrency={preferredCurrency}
                      fxRate={fxRates[e.fee_currency ?? e.currency]}
                    />
                  ) : "—"}
                </TableCell>
              )}
              {isVisible("currency") && (
                <TableCell className="text-muted-foreground">{e.currency}</TableCell>
              )}
              {isVisible("source") && (
                <TableCell>
                  <SourceChips documentIds={e.source_document_ids} docs={docsMap} />
                </TableCell>
              )}
              {isVisible("notes") && (
                <TableCell className="max-w-[200px] truncate text-sm text-muted-foreground" title={e.notes ?? undefined}>
                  {e.notes ?? "—"}
                </TableCell>
              )}
              <TableCell className="text-right">
                <div className="flex items-center justify-end gap-1">
                  <Button variant="ghost" size="icon" className="h-8 w-8 opacity-0 group-hover:opacity-100"
                    onClick={() => onEdit(e)} title="Edit event">
                    <Pencil className="h-3.5 w-3.5" />
                  </Button>
                  <Button variant="ghost" size="icon" className="h-8 w-8 opacity-0 group-hover:opacity-100"
                    onClick={() => onDelete(e)} title="Delete event">
                    <Trash2 className="h-3.5 w-3.5" />
                  </Button>
                </div>
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>

      <div className="flex items-center justify-between border-t px-2 py-3">
        <div className="flex items-center gap-3">
          <span className="text-sm text-muted-foreground">{total} events</span>
          <div className="flex items-center gap-1.5">
            <span className="text-xs text-muted-foreground">Show</span>
            <Select
              value={pageSize.toString()}
              onValueChange={(v) => {
                if (v == null) return
                onPageSizeChange(parseInt(v, 10))
              }}
            >
              <SelectTrigger className="h-7 w-[65px] text-xs">
                <span>{pageSize}</span>
              </SelectTrigger>
              <SelectContent>
                {PAGE_SIZE_OPTIONS.map((size) => (
                  <SelectItem key={size} value={size.toString()}>{size}</SelectItem>
                ))}
              </SelectContent>
            </Select>
            <span className="text-xs text-muted-foreground">per page</span>
          </div>
        </div>
        <div className="flex items-center gap-2">
          <span className="text-sm text-muted-foreground">Page {safePage} of {totalPages}</span>
          <div className="flex gap-1">
            <Button
              variant="outline"
              size="sm"
              className="h-7 w-7 p-0"
              disabled={safePage <= 1}
              onClick={() => onPageChange(safePage - 1)}
            >
              <ChevronLeft className="h-4 w-4" />
            </Button>
            <Button
              variant="outline"
              size="sm"
              className="h-7 w-7 p-0"
              disabled={safePage >= totalPages}
              onClick={() => onPageChange(safePage + 1)}
            >
              <ChevronRight className="h-4 w-4" />
            </Button>
          </div>
        </div>
      </div>
    </div>
  )
}

function SortableHeader({
  label, column, activeColumn, direction, onClick, align = "left", className,
}: {
  label: string
  column: InvSortColumn
  activeColumn: InvSortColumn | undefined
  direction: SortDir
  onClick: (col: InvSortColumn) => void
  align?: "left" | "right"
  className?: string
}) {
  const active = activeColumn === column
  const Icon = !active ? ArrowUpDown : direction === "asc" ? ArrowUp : ArrowDown
  return (
    <TableHead className={className}>
      <button
        type="button"
        onClick={() => onClick(column)}
        className={cn(
          "inline-flex items-center gap-1 select-none cursor-pointer rounded-md px-1 py-0.5 -mx-1 hover:bg-muted transition-colors",
          align === "right" && "flex-row-reverse w-full justify-start",
          active ? "text-foreground" : "text-muted-foreground hover:text-foreground"
        )}
        aria-label={`Sort by ${label}`}
      >
        <span>{label}</span>
        <Icon className={cn("h-3 w-3", active ? "opacity-100" : "opacity-50")} />
      </button>
    </TableHead>
  )
}

function ColumnSettings({
  columns, visible, onToggle,
}: {
  columns: Column[]
  visible: Set<string>
  onToggle: (id: string) => void
}) {
  return (
    <Popover>
      <PopoverTrigger className="inline-flex items-center justify-center rounded-md p-1 hover:bg-muted transition-colors">
        <Settings2 className="h-4 w-4 text-muted-foreground" />
      </PopoverTrigger>
      <PopoverContent className="w-[180px] p-2" align="end">
        <p className="text-xs font-medium text-muted-foreground mb-2">Visible columns</p>
        {columns.map((col) => (
          <button
            key={col.id}
            onClick={() => onToggle(col.id)}
            className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-sm hover:bg-muted transition-colors"
          >
            <Check className={`h-3.5 w-3.5 ${visible.has(col.id) ? "opacity-100" : "opacity-0"}`} />
            {col.label}
          </button>
        ))}
      </PopoverContent>
    </Popover>
  )
}

function num(s: string): number {
  const n = Number.parseFloat(s)
  return Number.isFinite(n) ? n : 0
}

function fmtQty(qty: string): string {
  const n = Number.parseFloat(qty)
  if (!Number.isFinite(n)) return qty
  return n.toLocaleString("en-GB", { minimumFractionDigits: 0, maximumFractionDigits: 4 })
}
