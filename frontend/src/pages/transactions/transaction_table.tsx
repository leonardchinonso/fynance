import { useState } from "react"
import type { Transaction } from "@/types"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { Badge } from "@/components/ui/badge"
import { Currency } from "@/components/currency"
import { formatDate } from "@/lib/utils"
import { Button } from "@/components/ui/button"
import { ChevronLeft, ChevronRight, Settings2, Check, EyeOff } from "lucide-react"
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
import { CATEGORY_COLORS } from "@/lib/colors"
import { Switch } from "@/components/ui/switch"
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip"

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

function getCategoryColor(category: string): string {
  const parent = category.split(":")[0].trim()
  return CATEGORY_COLORS[parent] ?? "#78716c"
}

interface TransactionTableProps {
  transactions: Transaction[]
  total: number
  page: number
  limit: number
  onPageChange: (page: number) => void
  onLimitChange: (limit: number) => void
  accountNames?: Record<string, string>
}

export function TransactionTable({
  transactions,
  total,
  page,
  limit,
  onPageChange,
  onLimitChange,
  accountNames = {},
}: TransactionTableProps) {
  const totalPages = Math.ceil(total / limit)
  const [visibleColumns, setVisibleColumns] = useState<Set<string>>(getStoredColumns)

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
            {isVisible("date") && <TableHead>Date</TableHead>}
            {isVisible("merchant") && <TableHead>Merchant</TableHead>}
            {isVisible("category") && <TableHead>Category</TableHead>}
            {isVisible("amount") && <TableHead className="text-right">Amount</TableHead>}
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
                  {t.category ? (
                    <Badge
                      variant="secondary"
                      className="text-xs"
                      style={{
                        backgroundColor: getCategoryColor(t.category) + "20",
                        color: getCategoryColor(t.category),
                        borderColor: getCategoryColor(t.category) + "40",
                      }}
                    >
                      {t.category}
                    </Badge>
                  ) : (
                    <Badge variant="outline" className="text-xs text-muted-foreground">
                      Uncategorized
                    </Badge>
                  )}
                </TableCell>
              )}
              {isVisible("amount") && (
                <TableCell className="text-right">
                  <Currency amount={t.amount} currency={t.currency} />
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
                  <TooltipProvider>
                    <Tooltip>
                      <TooltipTrigger asChild>
                        <span className="inline-flex">
                          <Switch disabled checked={false} className="scale-75" />
                        </span>
                      </TooltipTrigger>
                      <TooltipContent>
                        <p>Coming soon: exclude from summaries</p>
                      </TooltipContent>
                    </Tooltip>
                  </TooltipProvider>
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
