import { useMemo, useState } from "react"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { Button } from "@/components/ui/button"
import { Trash2, RotateCcw } from "lucide-react"
import { cn } from "@/lib/utils"
import type { TransactionIngestionResult } from "@/bindings/TransactionIngestionResult"
import type { ImportPayload } from "@/bindings/ImportPayload"
import type { ImportTransaction } from "@/bindings/ImportTransaction"
import type { Currency } from "@/types"
import { StatusBadge } from "./status_badge"
import { DateCell, DecimalCell, SelectCell, TextCell, moneyDirectionClass } from "./editors"
import { SectionShell, useSectionControls } from "./section_shell"
import { useCategoryColorsContext } from "@/context/category_colors_context"
import { CATEGORY_COLORS } from "@/lib/colors"
import { Input } from "@/components/ui/input"
import {
  Select as UiSelect,
  SelectContent as UiSelectContent,
  SelectItem as UiSelectItem,
  SelectTrigger as UiSelectTrigger,
  SelectValue as UiSelectValue,
} from "@/components/ui/select"

interface Props {
  result: TransactionIngestionResult
  payload: ImportPayload | null
  setPayload: (p: ImportPayload | null) => void
  markedForDeletion: Set<number>
  setMarkedForDeletion: (s: Set<number>) => void
  categoryOptions: string[]
  currencyOptions: Currency[]
}

export function TransactionsSection({
  result,
  payload,
  setPayload,
  markedForDeletion,
  setMarkedForDeletion,
  categoryOptions,
  currencyOptions,
}: Props) {
  const ctrls = useSectionControls()
  const { categoryColors } = useCategoryColorsContext()
  const [dateFrom, setDateFrom] = useState("")
  const [dateTo, setDateTo] = useState("")
  const [categoryFilter, setCategoryFilter] = useState("__all__")

  const availableCategories = useMemo(() => {
    const set = new Set<string>()
    payload?.transactions.forEach((t) => { if (t.category) set.add(t.category) })
    return [...set].sort()
  }, [payload])

  /**
   * Map each preview row to the index of its corresponding payload entry,
   * or null for non-actionable rows (duplicate/error).
   *
   * Backend invariant: payload.transactions is in the same order as
   * `rows[]` filtered to status === "new".
   */
  const rowPayloadIndex = useMemo(() => {
    let n = 0
    return result.rows.map((r) => (r.status === "new" ? n++ : null))
  }, [result.rows])

  const filteredEntries = useMemo(() => {
    const fromIso = dateFrom ? `${dateFrom}T00:00:00` : null
    const toIso = dateTo ? `${dateTo}T23:59:59` : null
    return result.rows
      .map((row, displayIdx) => ({ row, displayIdx, payloadIdx: rowPayloadIndex[displayIdx] }))
      .filter(({ row, payloadIdx }) => {
        if (fromIso && row.date < fromIso) return false
        if (toIso && row.date > toIso) return false
        if (categoryFilter !== "__all__") {
          const cat = payloadIdx !== null ? payload?.transactions[payloadIdx]?.category : null
          if (categoryFilter === "__none__" ? cat : cat !== categoryFilter) return false
        }
        return true
      })
  }, [result.rows, rowPayloadIndex, dateFrom, dateTo, categoryFilter, payload])

  const totalRows = filteredEntries.length
  const start = (ctrls.page - 1) * ctrls.pageSize
  const pageEntries = filteredEntries.slice(start, start + ctrls.pageSize)

  function updatePayloadAt(idx: number, patch: Partial<ImportTransaction>) {
    if (!payload) return
    const next: ImportTransaction[] = payload.transactions.map((t, i) =>
      i === idx ? { ...t, ...patch } : t
    )
    setPayload({ ...payload, transactions: next })
  }

  function toggleDelete(idx: number) {
    const next = new Set(markedForDeletion)
    if (next.has(idx)) next.delete(idx)
    else next.add(idx)
    setMarkedForDeletion(next)
  }

  const newCount = result.new
  const dupCount = result.duplicate
  const errCount = result.errors
  const willCommit = (payload?.transactions.length ?? 0) - markedForDeletion.size

  const summary = (
    <>
      {newCount} new · {dupCount} duplicate · {errCount} error
      {markedForDeletion.size > 0 && (
        <> · <span className="text-foreground">{markedForDeletion.size} marked skip</span></>
      )}
      <> · <span className="text-foreground">{Math.max(0, willCommit)} will commit</span></>
    </>
  )

  const categoryOpts = categoryOptions.map((c) => ({ value: c, label: c }))
  const currencyOpts = currencyOptions.map((c) => ({ value: c.code, label: c.code }))

  const filterSlot = (
    <>
      <Input
        type="date"
        value={dateFrom}
        onChange={(e) => { setDateFrom(e.target.value); ctrls.setPage(1) }}
        className="h-8 w-[9rem] text-xs"
        aria-label="From date"
      />
      <span className="text-xs text-muted-foreground">to</span>
      <Input
        type="date"
        value={dateTo}
        onChange={(e) => { setDateTo(e.target.value); ctrls.setPage(1) }}
        className="h-8 w-[9rem] text-xs"
        aria-label="To date"
      />
      <UiSelect
        value={categoryFilter}
        onValueChange={(v) => { if (v) { setCategoryFilter(v); ctrls.setPage(1) } }}
      >
        <UiSelectTrigger className="h-8 text-xs min-w-[10rem]">
          <UiSelectValue />
        </UiSelectTrigger>
        <UiSelectContent>
          <UiSelectItem value="__all__">All categories</UiSelectItem>
          <UiSelectItem value="__none__">Uncategorized</UiSelectItem>
          {availableCategories.map((c) => (
            <UiSelectItem key={c} value={c}>{c}</UiSelectItem>
          ))}
        </UiSelectContent>
      </UiSelect>
    </>
  )

  return (
    <SectionShell
      title="Transactions"
      summary={summary}
      filterSlot={filterSlot}
      totalRows={totalRows}
      pageSize={ctrls.pageSize}
      page={ctrls.page}
      onPageChange={ctrls.setPage}
      onPageSizeChange={(s) => {
        ctrls.setPageSize(s)
        ctrls.setPage(1)
      }}
    >
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead className="w-24">Action</TableHead>
            <TableHead className="w-36">Date</TableHead>
            <TableHead>Description</TableHead>
            <TableHead className="w-28 text-right">Amount</TableHead>
            <TableHead className="w-20">Currency</TableHead>
            <TableHead className="w-40">Category</TableHead>
            <TableHead className="w-10" />
          </TableRow>
        </TableHeader>
        <TableBody>
          {pageEntries.length === 0 && (
            <TableRow>
              <TableCell colSpan={7} className="text-center text-xs text-muted-foreground py-6">
                No rows match
              </TableCell>
            </TableRow>
          )}
          {pageEntries.map(({ row, displayIdx, payloadIdx }) => {
            const editable = payloadIdx !== null && payload !== null
            const tx = editable ? payload.transactions[payloadIdx] : null
            const marked = payloadIdx !== null && markedForDeletion.has(payloadIdx)
            return (
              <TableRow
                key={`${displayIdx}-${row.index}`}
                className={cn(
                  marked && "opacity-40 line-through",
                  !editable && "bg-muted/30 text-muted-foreground"
                )}
              >
                <TableCell>
                  <StatusBadge status={marked ? "duplicate" : row.status} />
                </TableCell>
                <TableCell>
                  {editable && tx && !marked ? (
                    <DateCell value={tx.date} onChange={(v) => updatePayloadAt(payloadIdx, { date: v })} />
                  ) : (
                    <span className="text-xs">{(row.date.split("T")[0] ?? row.date)}</span>
                  )}
                </TableCell>
                <TableCell>
                  {editable && tx && !marked ? (
                    <TextCell value={tx.description} onChange={(v) => updatePayloadAt(payloadIdx, { description: v })} />
                  ) : (
                    <span className="text-xs">
                      {row.description}
                      {row.existing_description && row.existing_description !== row.description && (
                        <span className="ml-2 text-muted-foreground">↳ matched: {row.existing_description}</span>
                      )}
                      {row.error_reason && (
                        <span className="ml-2 text-destructive">({row.error_reason})</span>
                      )}
                    </span>
                  )}
                </TableCell>
                <TableCell className="text-right">
                  {editable && tx && !marked ? (
                    <DecimalCell
                      value={tx.amount}
                      onChange={(v) => updatePayloadAt(payloadIdx, { amount: v })}
                      colorize
                    />
                  ) : (
                    <span className={cn("text-xs tabular-nums", moneyDirectionClass(row.amount))}>
                      {row.amount}
                    </span>
                  )}
                </TableCell>
                <TableCell>
                  {editable && tx && !marked ? (
                    <SelectCell
                      value={tx.currency}
                      options={currencyOpts}
                      onChange={(v) => updatePayloadAt(payloadIdx, { currency: v })}
                    />
                  ) : (
                    <span className="text-xs">{row.currency}</span>
                  )}
                </TableCell>
                <TableCell>
                  {editable && tx && !marked ? (
                    <SelectCell
                      value={tx.category}
                      options={categoryOpts}
                      onChange={(v) =>
                        updatePayloadAt(payloadIdx, { category: v, category_source: "manual" })
                      }
                      placeholder="Uncategorized"
                      tintColor={
                        tx.category
                          ? categoryColors[tx.category] ?? CATEGORY_COLORS[tx.category]
                          : undefined
                      }
                    />
                  ) : (
                    <span className="text-xs text-muted-foreground">—</span>
                  )}
                </TableCell>
                <TableCell>
                  {editable && payloadIdx !== null && (
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-7 w-7"
                      onClick={() => toggleDelete(payloadIdx)}
                      aria-label={marked ? "Unmark deletion" : "Mark for deletion"}
                    >
                      {marked ? <RotateCcw className="h-3.5 w-3.5" /> : <Trash2 className="h-3.5 w-3.5" />}
                    </Button>
                  )}
                </TableCell>
              </TableRow>
            )
          })}
        </TableBody>
      </Table>
    </SectionShell>
  )
}
