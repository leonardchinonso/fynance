import { useEffect, useMemo } from "react"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { Button } from "@/components/ui/button"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { SourceChips, type SourceDocMeta } from "@/components/source_chips"
import { Trash2, RotateCcw, Undo2 } from "lucide-react"
import { cn, formatDate } from "@/lib/utils"
import type { TransactionIngestionResult } from "@/bindings/TransactionIngestionResult"
import type { ImportPayload } from "@/bindings/ImportPayload"
import type { ImportTransaction } from "@/bindings/ImportTransaction"
import type { CategorySource } from "@/bindings/CategorySource"
import type { Currency } from "@/types"
import { StatusBadge } from "./status_badge"
import { DateCell, DecimalCell, SelectCell, TextCell, moneyDirectionClass } from "./editors"
import { SectionShell, useSectionControls } from "./section_shell"
import { SortHeader } from "./sort_header"
import { useCategoryColorsContext } from "@/context/category_colors_context"
import { CATEGORY_COLORS } from "@/lib/colors"
import { InlineDateRange } from "@/components/inline_date_range"
import { useUrlState } from "@/hooks/use_url_state"
import {
  Select as UiSelect,
  SelectContent as UiSelectContent,
  SelectItem as UiSelectItem,
  SelectTrigger as UiSelectTrigger,
} from "@/components/ui/select"

const TX_CATEGORY_LABELS: Record<string, string> = {
  __all__: "All categories",
  __none__: "Uncategorized",
}

const STATUS_RANK: Record<string, number> = {
  new: 0,
  modify: 1,
  duplicate: 2,
  error: 3,
  removed: 4,
}

interface EntryShape {
  row: {
    date: string
    amount: string
    currency: string
    status: string
    category_confidence?: number | null
  }
  payloadIdx: number | null
}

function sortValue(
  entry: EntryShape,
  column: string,
  payload: ImportPayload | null,
  marked: Set<number>
): string | number {
  // For edited rows, prefer the value the user actually sees. The display
  // mutates payload.transactions[payloadIdx] while row.* stays at the
  // original parsed value, so sorting by row.* alone made edited rows
  // appear stuck in their original position.
  const edited = entry.payloadIdx !== null ? payload?.transactions[entry.payloadIdx] : undefined
  switch (column) {
    case "date":
      return edited?.date ?? entry.row.date
    case "amount": {
      const raw = edited?.amount ?? entry.row.amount
      const n = parseFloat(raw)
      return Number.isFinite(n) ? n : 0
    }
    case "action": {
      const status = entry.payloadIdx !== null && marked.has(entry.payloadIdx) ? "removed" : entry.row.status
      return STATUS_RANK[status] ?? 99
    }
    default:
      return 0
  }
}

function parentNameFromDisplay(displayName: string | null | undefined): string | null {
  if (!displayName) return null
  const idx = displayName.indexOf(":")
  return idx > 0 ? displayName.slice(0, idx).trim() : displayName.trim()
}

function confidenceWording(value: number): string {
  if (value >= 0.9) {
    return "Agent is highly confident in this category. Skim to confirm, no deep review needed."
  }
  if (value >= 0.7) {
    return "Agent is reasonably confident, but a quick sanity check is worth it."
  }
  return "Agent is unsure about this category. Review carefully and pick the right one."
}

function confidenceColorClass(value: number): string {
  if (value >= 0.9) return "text-emerald-600 dark:text-emerald-400"
  if (value >= 0.7) return "text-amber-600 dark:text-amber-400"
  return "text-rose-600 dark:text-rose-400"
}

interface Props {
  result: TransactionIngestionResult
  payload: ImportPayload | null
  setPayload: (p: ImportPayload | null) => void
  markedForDeletion: Set<number>
  setMarkedForDeletion: (s: Set<number>) => void
  /** `category_id` → "Parent: Child" display name. */
  categoryById: Record<string, string>
  currencyOptions: Currency[]
  docs: Map<string, SourceDocMeta>
}

function nameFromId(
  categoryId: string | null | undefined,
  categoryById: Record<string, string>
): string | null {
  if (!categoryId) return null
  return categoryById[categoryId] ?? null
}

export function TransactionsSection({
  result,
  payload,
  setPayload,
  markedForDeletion,
  setMarkedForDeletion,
  categoryById,
  currencyOptions,
  docs,
}: Props) {
  const ctrls = useSectionControls()
  const { categoryColors, syncParents } = useCategoryColorsContext()
  const url = useUrlState()

  // Ensure parent-category colors are populated for tinting the Category select.
  // Without this, fresh sessions (no localStorage) leave the trigger ungoosed.
  useEffect(() => {
    const parents = new Set<string>()
    for (const name of Object.values(categoryById)) {
      const p = parentNameFromDisplay(name)
      if (p) parents.add(p)
    }
    if (parents.size > 0) syncParents([...parents])
  }, [categoryById, syncParents])

  const { minDate, maxDate } = useMemo(() => {
    const dates = result.rows
      .map((r) => r.date.split("T")[0] ?? "")
      .filter(Boolean)
      .sort()
    return { minDate: dates[0] ?? "", maxDate: dates[dates.length - 1] ?? "" }
  }, [result.rows])

  const dateFrom = url.get("txDateFrom", minDate)
  const dateTo = url.get("txDateTo", maxDate)
  const categoryFilter = url.get("txCat", "__all__")
  const sortColumn = url.get("txSort", "")
  const sortDir = (url.get("txDir", "asc") === "desc" ? "desc" : "asc") as "asc" | "desc"

  function setDateRange(start: string, end: string) {
    url.set({
      txDateFrom: start === minDate ? null : start,
      txDateTo: end === maxDate ? null : end,
    })
    ctrls.setPage(1)
  }
  function setCategoryFilter(v: string) {
    url.set({ txCat: v === "__all__" ? null : v })
    ctrls.setPage(1)
  }
  function cycleSort(col: string) {
    if (sortColumn !== col) {
      url.set({ txSort: col, txDir: null })
    } else if (sortDir === "asc") {
      url.set({ txSort: col, txDir: "desc" })
    } else {
      url.set({ txSort: null, txDir: null })
    }
  }

  // Limit the filter dropdown to ids actually present on the payload.
  const availableCategoryIds = useMemo(() => {
    const set = new Set<string>()
    payload?.transactions.forEach((t) => {
      if (t.category_id) set.add(t.category_id)
    })
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
    const filtered = result.rows
      .map((row, displayIdx) => ({ row, displayIdx, payloadIdx: rowPayloadIndex[displayIdx] }))
      .filter(({ row, payloadIdx }) => {
        if (fromIso && row.date < fromIso) return false
        if (toIso && row.date > toIso) return false
        if (categoryFilter !== "__all__") {
          const id = payloadIdx !== null
            ? payload?.transactions[payloadIdx]?.category_id ?? null
            : row.category_id ?? null
          if (categoryFilter === "__none__" ? id : id !== categoryFilter) return false
        }
        return true
      })
    if (!sortColumn) return filtered
    const sign = sortDir === "asc" ? 1 : -1
    const sorted = [...filtered].sort((a, b) => {
      const ax = sortValue(a, sortColumn, payload, markedForDeletion)
      const bx = sortValue(b, sortColumn, payload, markedForDeletion)
      if (ax < bx) return -1 * sign
      if (ax > bx) return 1 * sign
      return 0
    })
    return sorted
  }, [result.rows, rowPayloadIndex, dateFrom, dateTo, categoryFilter, payload, sortColumn, sortDir, markedForDeletion])

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

  const categoryIdOpts = useMemo(
    () =>
      Object.entries(categoryById)
        .map(([id, name]) => ({ value: id, label: name }))
        .sort((a, b) => a.label.localeCompare(b.label)),
    [categoryById]
  )

  const summary = (
    <>
      {newCount} new · {dupCount} duplicate · {errCount} error
      {markedForDeletion.size > 0 && (
        <> · <span className="text-foreground">{markedForDeletion.size} marked skip</span></>
      )}
      <> · <span className="text-foreground">{Math.max(0, willCommit)} will commit</span></>
    </>
  )

  const currencyOpts = currencyOptions.map((c) => ({ value: c.code, label: c.code }))

  const filterSlot = (
    <>
      <InlineDateRange
        start={dateFrom}
        end={dateTo}
        onChange={({ start, end }) => setDateRange(start, end)}
      />
      <UiSelect
        value={categoryFilter}
        onValueChange={(v) => { if (v) setCategoryFilter(v) }}
      >
        <UiSelectTrigger className="h-8 text-xs min-w-[10rem]">
          <span>
            {TX_CATEGORY_LABELS[categoryFilter] ??
              categoryById[categoryFilter] ??
              categoryFilter}
          </span>
        </UiSelectTrigger>
        <UiSelectContent>
          <UiSelectItem value="__all__">All categories</UiSelectItem>
          <UiSelectItem value="__none__">Uncategorized</UiSelectItem>
          {availableCategoryIds.map((id) => (
            <UiSelectItem key={id} value={id}>
              {categoryById[id] ?? id}
            </UiSelectItem>
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
            <SortHeader label="Action" columnId="action" activeColumn={sortColumn} direction={sortDir} onClick={() => cycleSort("action")} className="w-24" />
            <SortHeader label="Date" columnId="date" activeColumn={sortColumn} direction={sortDir} onClick={() => cycleSort("date")} className="w-36" />
            <TableHead>Description</TableHead>
            <SortHeader label="Amount" columnId="amount" activeColumn={sortColumn} direction={sortDir} onClick={() => cycleSort("amount")} className="w-28" align="right" />
            <TableHead className="w-20">Currency</TableHead>
            <TableHead className="w-56">Category</TableHead>
            <TableHead className="w-20">Source</TableHead>
            <TableHead className="w-10" />
          </TableRow>
        </TableHeader>
        <TableBody>
          {pageEntries.length === 0 && (
            <TableRow>
              <TableCell colSpan={8} className="text-center text-xs text-muted-foreground py-6">
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
                  <StatusBadge
                    status={marked ? "removed" : row.status}
                    errorMessage={row.error_reason}
                  />
                </TableCell>
                <TableCell>
                  {editable && tx && !marked ? (
                    <DateCell value={tx.date} onChange={(v) => updatePayloadAt(payloadIdx, { date: v })} />
                  ) : (
                    <span className="text-xs text-muted-foreground tabular-nums">{formatDate(row.date.split("T")[0] ?? row.date)}</span>
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
                  {editable && tx && !marked ? (() => {
                    const displayName = nameFromId(tx.category_id, categoryById)
                    const parent = parentNameFromDisplay(displayName)
                    const tintColor = parent
                      ? categoryColors[parent] ?? CATEGORY_COLORS[parent]
                      : undefined
                    const agentPickId = row.category_id
                    const conf = row.category_confidence
                    const isAgentPick =
                      agentPickId != null && tx.category_id === agentPickId
                    const userOverrode =
                      agentPickId != null && tx.category_id !== agentPickId
                    return (
                      <div className="flex items-center gap-1.5">
                        <SelectCell
                          value={tx.category_id ?? null}
                          options={categoryIdOpts}
                          onChange={(v) =>
                            updatePayloadAt(payloadIdx, {
                              category_id: v || null,
                              category_source: "manual" satisfies CategorySource,
                            })
                          }
                          placeholder="Uncategorized"
                          tintColor={tintColor}
                        />
                        {isAgentPick && conf != null && (
                          <Tooltip>
                            <TooltipTrigger
                              render={
                                <span
                                  className={cn(
                                    "text-[11px] tabular-nums cursor-help",
                                    confidenceColorClass(conf)
                                  )}
                                >
                                  {Math.round(conf * 100)}%
                                </span>
                              }
                            />
                            <TooltipContent>{confidenceWording(conf)}</TooltipContent>
                          </Tooltip>
                        )}
                        {userOverrode && (
                          <Tooltip>
                            <TooltipTrigger
                              render={
                                <Button
                                  variant="ghost"
                                  size="icon"
                                  className="h-6 w-6 text-muted-foreground hover:text-foreground"
                                  onClick={() =>
                                    updatePayloadAt(payloadIdx, {
                                      category_id: agentPickId,
                                      category_source: "agent" satisfies CategorySource,
                                    })
                                  }
                                  aria-label="Reset to agent's category"
                                >
                                  <Undo2 className="h-3 w-3" />
                                </Button>
                              }
                            />
                            <TooltipContent>
                              Reset to agent's pick (
                              {nameFromId(agentPickId, categoryById) ?? agentPickId})
                              {conf != null && ` · ${Math.round(conf * 100)}%`}
                            </TooltipContent>
                          </Tooltip>
                        )}
                      </div>
                    )
                  })() : (() => {
                    const displayName = nameFromId(row.category_id, categoryById)
                    return displayName ? (
                      <span className="text-xs">{displayName}</span>
                    ) : (
                      <span className="text-xs text-muted-foreground">—</span>
                    )
                  })()}
                </TableCell>
                <TableCell>
                  <SourceChips documentIds={row.source_document_ids} docs={docs} />
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
