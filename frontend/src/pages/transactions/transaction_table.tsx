import { useState, useEffect, type ReactNode } from "react"
import type { Transaction } from "@/types"
import { api } from "@/api/client"
import type { RemoteData } from "@/lib/remote_data"
import { visitRemoteData } from "@/lib/remote_data"
import type { TransactionsData } from "@/hooks/data"
import { TableSkeleton } from "@/components/skeletons"
import { AuthAwareError } from "@/components/auth_aware_error"
import { ReloadingOverlay } from "@/components/reloading_overlay"
import { EmptyState } from "@/components/empty_state"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { Badge } from "@/components/ui/badge"
import { MoneyDisplay } from "@/components/currency"
import { formatDate } from "@/lib/utils"
import { Button } from "@/components/ui/button"
import { ChevronLeft, ChevronRight, Settings2, Check, ArrowUp, ArrowDown, ArrowUpDown } from "lucide-react"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
} from "@/components/ui/select"
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover"
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command"
import { CATEGORY_COLORS } from "@/lib/colors"
import { Switch } from "@/components/ui/switch"
import { cn } from "@/lib/utils"
import type { SortDir, TransactionSortColumn } from "@/types"

const PAGE_SIZE_OPTIONS = [10, 25, 50, 100]
const PAGE_SIZE_KEY = "fynance-page-size"
const COLUMNS_KEY = "fynance-tx-columns"

interface Column {
  id: string
  label: string
  defaultVisible: boolean
}

const ALL_COLUMNS: Column[] = [
  { id: "date", label: "Date", defaultVisible: true },
  { id: "merchant", label: "Merchant", defaultVisible: true },
  { id: "category", label: "Category", defaultVisible: true },
  { id: "amount", label: "Amount", defaultVisible: true },
  { id: "account", label: "Account", defaultVisible: true },
  { id: "source", label: "Source", defaultVisible: false },
  { id: "exclude", label: "Exclude", defaultVisible: false },
]

function getStoredColumns(): Set<string> {
  try {
    const v = localStorage.getItem(COLUMNS_KEY)
    if (v) return new Set(JSON.parse(v))
  } catch { /* ignore */ }
  return new Set(ALL_COLUMNS.filter((c) => c.defaultVisible).map((c) => c.id))
}

function getCategoryColor(category: string, colorMap: Record<string, string>): string {
  const parent = category.split(":")[0].trim()
  return colorMap[parent] ?? CATEGORY_COLORS[parent] ?? "#78716c"
}

interface TransactionTableOuterProps {
  data: RemoteData<TransactionsData>
  page: number
  pageSize: number
  onPageChange: (page: number) => void
  onPageSizeChange: (size: number) => void
  accountNames: Record<string, string>
  categoryColors?: Record<string, string>
  /** Categories available for the inline-edit popover. Empty until loaded. */
  categoryOptions?: Array<{ id: string; name: string }>
  sort?: TransactionSortColumn
  sortDir: SortDir
  onSort: (col: TransactionSortColumn) => void
  onResetFilters?: () => void
}

export function TransactionTable({
  data, page, pageSize, onPageChange, onPageSizeChange, accountNames, categoryColors = {},
  categoryOptions = [], sort, sortDir, onSort, onResetFilters,
}: TransactionTableOuterProps) {
  return visitRemoteData(data, {
    notLoaded: () => <TableSkeleton rows={25} cols={5} />,
    failed: (error) => <AuthAwareError error={error} />,
    hasValue: ({ result }) => (
      <div className="relative">
        {result.data.length === 0 ? (
          <EmptyState action={onResetFilters ? { label: "Reset filters", onClick: onResetFilters } : undefined} />
        ) : (
          <TransactionTableInternal
            transactions={result.data}
            total={result.total}
            page={page}
            limit={pageSize}
            onPageChange={onPageChange}
            onLimitChange={onPageSizeChange}
            accountNames={accountNames}
            categoryColors={categoryColors}
            categoryOptions={categoryOptions}
            sort={sort}
            sortDir={sortDir}
            onSort={onSort}
          />
        )}
        <ReloadingOverlay active={data.status === "reloading"} />
      </div>
    ),
  })
}

interface TransactionTableProps {
  transactions: Transaction[]
  total: number
  page: number
  limit: number
  onPageChange: (page: number) => void
  onLimitChange: (limit: number) => void
  accountNames?: Record<string, string>
  categoryColors?: Record<string, string>
  categoryOptions: Array<{ id: string; name: string }>
  sort?: TransactionSortColumn
  sortDir: SortDir
  onSort: (col: TransactionSortColumn) => void
}

/**
 * Inline category editor. The trigger is the existing Badge so the rest of
 * the row layout doesn't shift. Selecting a category PATCHes the transaction
 * (backend sets category_source=manual automatically) and is applied
 * optimistically — failures roll back via the parent's local state.
 */
function CategoryEditPopover({
  current,
  triggerLabel,
  triggerColor,
  options,
  onSelect,
  disabled,
}: {
  current: string | null
  triggerLabel: ReactNode
  triggerColor: string
  options: Array<{ id: string; name: string }>
  onSelect: (name: string) => void
  disabled?: boolean
}) {
  const [open, setOpen] = useState(false)
  const sorted = [...options].sort((a, b) => a.name.localeCompare(b.name))
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger
        disabled={disabled}
        className={cn(
          "rounded-md",
          "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
          disabled && "cursor-not-allowed opacity-60",
          !disabled && "cursor-pointer"
        )}
      >
        <Badge
          variant={current ? "secondary" : "outline"}
          className={cn("text-xs", !current && "text-muted-foreground")}
          style={
            current
              ? {
                  backgroundColor: triggerColor + "20",
                  color: triggerColor,
                  borderColor: triggerColor + "40",
                }
              : undefined
          }
        >
          {triggerLabel}
        </Badge>
      </PopoverTrigger>
      <PopoverContent className="w-[260px] p-0" align="start">
        <Command>
          <CommandInput placeholder="Search categories..." className="h-9 text-xs" />
          <CommandList className="max-h-[280px]">
            <CommandEmpty>No matches.</CommandEmpty>
            <CommandGroup>
              {sorted.map((opt) => {
                const isCurrent = opt.name === current
                return (
                  <CommandItem
                    key={opt.id}
                    value={opt.name}
                    onSelect={() => {
                      if (!isCurrent) onSelect(opt.name)
                      setOpen(false)
                    }}
                  >
                    <Check className={cn("mr-2 h-3.5 w-3.5", isCurrent ? "opacity-100" : "opacity-0")} />
                    {opt.name}
                  </CommandItem>
                )
              })}
            </CommandGroup>
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  )
}

function SortableHeader({
  label,
  column,
  activeColumn,
  direction,
  onClick,
  align = "left",
  className,
}: {
  label: string
  column: TransactionSortColumn
  activeColumn: TransactionSortColumn | undefined
  direction: SortDir
  onClick: (col: TransactionSortColumn) => void
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

function TransactionTableInternal({
  transactions: initialTransactions,
  total,
  page,
  limit,
  onPageChange,
  onLimitChange,
  accountNames = {},
  categoryColors = {},
  categoryOptions,
  sort,
  sortDir,
  onSort,
}: TransactionTableProps) {
  const totalPages = Math.ceil(total / limit)
  const [visibleColumns, setVisibleColumns] = useState<Set<string>>(getStoredColumns)
  const [transactions, setTransactions] = useState(initialTransactions)

  useEffect(() => {
    setTransactions(initialTransactions)
  }, [initialTransactions])

  async function toggleExclude(id: string, current: boolean) {
    setTransactions(prev => prev.map(t => t.id === id ? { ...t, exclude_from_summary: !current } : t))
    try {
      await api.patchTransaction(id, { exclude_from_summary: !current })
    } catch {
      setTransactions(prev => prev.map(t => t.id === id ? { ...t, exclude_from_summary: current } : t))
    }
  }

  async function changeCategory(id: string, previousCategory: string | null, nextCategoryName: string) {
    setTransactions(prev =>
      prev.map(t =>
        t.id === id ? { ...t, category: nextCategoryName, category_source: "manual" } : t
      )
    )
    try {
      const updated = await api.patchTransaction(id, { category: nextCategoryName })
      // Refresh from server in case the response normalises any fields.
      setTransactions(prev => prev.map(t => (t.id === id ? updated : t)))
    } catch {
      setTransactions(prev =>
        prev.map(t => (t.id === id ? { ...t, category: previousCategory } : t))
      )
    }
  }

  function toggleColumn(colId: string) {
    setVisibleColumns((prev) => {
      const next = new Set(prev)
      if (next.has(colId)) {
        next.delete(colId)
      } else {
        next.add(colId)
      }
      localStorage.setItem(COLUMNS_KEY, JSON.stringify(Array.from(next)))
      return next
    })
  }

  const isVisible = (colId: string) => visibleColumns.has(colId)

  return (
    <div>
      <Table>
        <TableHeader>
          <TableRow>
            {isVisible("date") && (
              <SortableHeader label="Date" column="date" activeColumn={sort} direction={sortDir} onClick={onSort} />
            )}
            {isVisible("merchant") && <TableHead>Merchant</TableHead>}
            {isVisible("category") && (
              <SortableHeader label="Category" column="category" activeColumn={sort} direction={sortDir} onClick={onSort} />
            )}
            {isVisible("amount") && (
              <SortableHeader label="Amount" column="amount" activeColumn={sort} direction={sortDir} onClick={onSort} align="right" className="text-right" />
            )}
            {isVisible("account") && <TableHead>Account</TableHead>}
            {isVisible("source") && <TableHead>Source</TableHead>}
            {isVisible("exclude") && <TableHead className="text-center">Exclude</TableHead>}
            <TableHead className="w-8">
              <ColumnSettings
                columns={ALL_COLUMNS}
                visible={visibleColumns}
                onToggle={toggleColumn}
              />
            </TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {transactions.map((t) => (
            <TableRow key={t.id}>
              {isVisible("date") && (
                <TableCell className="whitespace-nowrap">
                  {formatDate(t.date)}
                </TableCell>
              )}
              {isVisible("merchant") && (
                <TableCell>{t.normalized}</TableCell>
              )}
              {isVisible("category") && (
                <TableCell>
                  <CategoryEditPopover
                    current={t.category}
                    triggerLabel={t.category ?? "Uncategorized"}
                    triggerColor={t.category ? getCategoryColor(t.category, categoryColors) : "#78716c"}
                    options={categoryOptions}
                    onSelect={(name) => changeCategory(t.id, t.category, name)}
                    disabled={categoryOptions.length === 0}
                  />
                </TableCell>
              )}
              {isVisible("amount") && (
                <TableCell className="text-right">
                  <MoneyDisplay amount={t.amount} currency={t.currency} />
                </TableCell>
              )}
              {isVisible("account") && (
                <TableCell className="text-sm text-muted-foreground">
                  {accountNames[t.account_id] ?? t.account_id}
                </TableCell>
              )}
              {isVisible("source") && (
                <TableCell>
                  {t.category_source === "agent" && t.confidence && (
                    <Badge variant="outline" className="text-xs">
                      AI {Math.round(t.confidence * 100)}%
                    </Badge>
                  )}
                  {t.category_source === "manual" && (
                    <Badge variant="outline" className="text-xs">
                      Manual
                    </Badge>
                  )}
                </TableCell>
              )}
              {isVisible("exclude") && (
                <TableCell className="text-center">
                  <Switch
                    checked={t.exclude_from_summary}
                    onCheckedChange={() => toggleExclude(t.id, t.exclude_from_summary)}
                    className="scale-75"
                  />
                </TableCell>
              )}
              <TableCell />
            </TableRow>
          ))}
        </TableBody>
      </Table>

      {/* Pagination */}
      <div className="flex items-center justify-between border-t px-2 py-3">
        <div className="flex items-center gap-3">
          <span className="text-sm text-muted-foreground">
            {total} transactions
          </span>
          <div className="flex items-center gap-1.5">
            <span className="text-xs text-muted-foreground">Show</span>
            <Select
              value={limit.toString()}
              onValueChange={(v) => {
                if (v == null){
                  return;
                }
                const newLimit = parseInt(v, 10)
                localStorage.setItem(PAGE_SIZE_KEY, v)
                onLimitChange(newLimit)
              }}
            >
              <SelectTrigger className="h-7 w-[65px] text-xs">
                <span>{limit}</span>
              </SelectTrigger>
              <SelectContent>
                {PAGE_SIZE_OPTIONS.map((size) => (
                  <SelectItem key={size} value={size.toString()}>
                    {size}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <span className="text-xs text-muted-foreground">per page</span>
          </div>
        </div>
        <div className="flex items-center gap-2">
          <span className="text-sm text-muted-foreground">
            Page {page} of {totalPages}
          </span>
          <div className="flex gap-1">
            <Button
              variant="outline"
              size="sm"
              className="h-7 w-7 p-0"
              disabled={page <= 1}
              onClick={() => onPageChange(page - 1)}
            >
              <ChevronLeft className="h-4 w-4" />
            </Button>
            <Button
              variant="outline"
              size="sm"
              className="h-7 w-7 p-0"
              disabled={page >= totalPages}
              onClick={() => onPageChange(page + 1)}
            >
              <ChevronRight className="h-4 w-4" />
            </Button>
          </div>
        </div>
      </div>
    </div>
  )
}

function ColumnSettings({
  columns,
  visible,
  onToggle,
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
        <p className="text-xs font-medium text-muted-foreground mb-2">
          Visible columns
        </p>
        {columns.map((col) => (
          <button
            key={col.id}
            onClick={() => onToggle(col.id)}
            className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-sm hover:bg-muted transition-colors"
          >
            <Check
              className={`h-3.5 w-3.5 ${visible.has(col.id) ? "opacity-100" : "opacity-0"}`}
            />
            {col.label}
          </button>
        ))}
      </PopoverContent>
    </Popover>
  )
}
