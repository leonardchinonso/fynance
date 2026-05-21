import { useMemo } from "react"
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
import { cn, formatDate } from "@/lib/utils"
import type { InvestmentIngestionResult } from "@/bindings/InvestmentIngestionResult"
import type { InvestmentsImportPayload } from "@/bindings/InvestmentsImportPayload"
import type { CreateInvestmentEventBody } from "@/bindings/CreateInvestmentEventBody"
import type { InvestmentEventType } from "@/bindings/InvestmentEventType"
import type { Currency } from "@/types"
import { StatusBadge } from "./status_badge"
import { DateCell, DecimalCell, SelectCell, TextCell } from "./editors"
import { SectionShell, useSectionControls } from "./section_shell"
import { SortHeader } from "./sort_header"
import { InlineDateRange } from "@/components/inline_date_range"
import { useUrlState } from "@/hooks/use_url_state"
import {
  Select as UiSelect,
  SelectContent as UiSelectContent,
  SelectItem as UiSelectItem,
  SelectTrigger as UiSelectTrigger,
} from "@/components/ui/select"

const EVENT_TYPES: InvestmentEventType[] = [
  "buy", "sell", "vest", "transfer", "withhold", "split",
]

const STATUS_RANK: Record<string, number> = {
  new: 0,
  modify: 1,
  duplicate: 2,
  error: 3,
  removed: 4,
}

interface EntryShape {
  row: { date: string; symbol: string; event_type: string; currency: string; status: string }
  payloadIdx: number | null
}

function invSortValue(
  entry: EntryShape,
  column: string,
  payload: InvestmentsImportPayload | null,
  marked: Set<number>
): string | number {
  const edited = entry.payloadIdx !== null ? payload?.events[entry.payloadIdx] : undefined
  switch (column) {
    case "date":
      return edited?.date ?? entry.row.date
    case "action": {
      const s = entry.payloadIdx !== null && marked.has(entry.payloadIdx) ? "removed" : entry.row.status
      return STATUS_RANK[s] ?? 99
    }
    case "event":
      return edited?.event_type ?? entry.row.event_type
    case "symbol":
      return edited?.symbol ?? entry.row.symbol
    case "currency":
      return edited?.currency ?? entry.row.currency
    default:
      return 0
  }
}

/** Cash-out events → red, cash-in events → green, structural events → neutral. */
function priceColorClass(eventType: string): string {
  switch (eventType) {
    case "buy":
    case "withhold":
      return "text-red-500"
    case "sell":
    case "vest":
      return "text-green-500"
    default:
      return ""
  }
}

interface Props {
  result: InvestmentIngestionResult
  payload: InvestmentsImportPayload | null
  setPayload: (p: InvestmentsImportPayload | null) => void
  markedForDeletion: Set<number>
  setMarkedForDeletion: (s: Set<number>) => void
  currencyOptions: Currency[]
}

export function InvestmentsSection({
  result,
  payload,
  setPayload,
  markedForDeletion,
  setMarkedForDeletion,
  currencyOptions,
}: Props) {
  const ctrls = useSectionControls()
  const url = useUrlState()

  const { minDate, maxDate } = useMemo(() => {
    const dates = result.rows
      .map((r) => r.date.split("T")[0] ?? "")
      .filter(Boolean)
      .sort()
    return { minDate: dates[0] ?? "", maxDate: dates[dates.length - 1] ?? "" }
  }, [result.rows])

  const dateFrom = url.get("invDateFrom", minDate)
  const dateTo = url.get("invDateTo", maxDate)
  const eventFilter = url.get("invEvent", "__all__")
  const sortColumn = url.get("invSort", "")
  const sortDir = (url.get("invDir", "asc") === "desc" ? "desc" : "asc") as "asc" | "desc"

  function setDateRange(start: string, end: string) {
    url.set({
      invDateFrom: start === minDate ? null : start,
      invDateTo: end === maxDate ? null : end,
    })
    ctrls.setPage(1)
  }
  function setEventFilter(v: string) {
    url.set({ invEvent: v === "__all__" ? null : v })
    ctrls.setPage(1)
  }
  function cycleSort(col: string) {
    if (sortColumn !== col) {
      url.set({ invSort: col, invDir: null })
    } else if (sortDir === "asc") {
      url.set({ invSort: col, invDir: "desc" })
    } else {
      url.set({ invSort: null, invDir: null })
    }
  }

  /**
   * Investment payload only contains rows with status === "new" in the
   * same order as `rows[]`.
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
      .filter(({ row }) => {
        if (fromIso && row.date < fromIso) return false
        if (toIso && row.date > toIso) return false
        if (eventFilter !== "__all__" && row.event_type !== eventFilter) return false
        return true
      })
    if (!sortColumn) return filtered
    const sign = sortDir === "asc" ? 1 : -1
    return [...filtered].sort((a, b) => {
      const av = invSortValue(a, sortColumn, payload, markedForDeletion)
      const bv = invSortValue(b, sortColumn, payload, markedForDeletion)
      if (av < bv) return -1 * sign
      if (av > bv) return 1 * sign
      return 0
    })
  }, [result.rows, rowPayloadIndex, dateFrom, dateTo, eventFilter, sortColumn, sortDir, markedForDeletion, payload])

  const totalRows = filteredEntries.length
  const start = (ctrls.page - 1) * ctrls.pageSize
  const pageEntries = filteredEntries.slice(start, start + ctrls.pageSize)

  function updatePayloadAt(idx: number, patch: Partial<CreateInvestmentEventBody>) {
    if (!payload) return
    const next: CreateInvestmentEventBody[] = payload.events.map((e, i) =>
      i === idx ? { ...e, ...patch } : e
    )
    setPayload({ ...payload, events: next })
  }

  function toggleDelete(idx: number) {
    const next = new Set(markedForDeletion)
    if (next.has(idx)) next.delete(idx)
    else next.add(idx)
    setMarkedForDeletion(next)
  }

  const willCommit = (payload?.events.length ?? 0) - markedForDeletion.size

  const summary = (
    <>
      {result.new} new · {result.duplicate} duplicate
      {markedForDeletion.size > 0 && (
        <> · <span className="text-foreground">{markedForDeletion.size} marked skip</span></>
      )}
      <> · <span className="text-foreground">{Math.max(0, willCommit)} will commit</span></>
    </>
  )

  const eventTypeOpts = EVENT_TYPES.map((t) => ({ value: t, label: t }))
  const currencyOpts = currencyOptions.map((c) => ({ value: c.code, label: c.code }))

  const filterSlot = (
    <>
      <InlineDateRange
        start={dateFrom}
        end={dateTo}
        onChange={({ start, end }) => setDateRange(start, end)}
      />
      <UiSelect
        value={eventFilter}
        onValueChange={(v) => { if (v) setEventFilter(v) }}
      >
        <UiSelectTrigger className="h-8 text-xs min-w-[8rem]">
          <span className="capitalize">
            {eventFilter === "__all__" ? "All events" : eventFilter}
          </span>
        </UiSelectTrigger>
        <UiSelectContent>
          <UiSelectItem value="__all__">All events</UiSelectItem>
          {EVENT_TYPES.map((t) => (
            <UiSelectItem key={t} value={t} className="capitalize">{t}</UiSelectItem>
          ))}
        </UiSelectContent>
      </UiSelect>
    </>
  )

  return (
    <SectionShell
      title="Investments"
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
            <SortHeader label="Event" columnId="event" activeColumn={sortColumn} direction={sortDir} onClick={() => cycleSort("event")} className="w-28" />
            <SortHeader label="Symbol" columnId="symbol" activeColumn={sortColumn} direction={sortDir} onClick={() => cycleSort("symbol")} className="w-28" />
            <SortHeader label="Date" columnId="date" activeColumn={sortColumn} direction={sortDir} onClick={() => cycleSort("date")} className="w-36" />
            <TableHead className="w-24 text-right">Qty</TableHead>
            <TableHead className="w-28 text-right">Price</TableHead>
            <TableHead className="w-24 text-right">Fee</TableHead>
            <SortHeader label="Currency" columnId="currency" activeColumn={sortColumn} direction={sortDir} onClick={() => cycleSort("currency")} className="w-20" />
            <TableHead className="w-10" />
          </TableRow>
        </TableHeader>
        <TableBody>
          {pageEntries.length === 0 && (
            <TableRow>
              <TableCell colSpan={9} className="text-center text-xs text-muted-foreground py-6">
                No rows match
              </TableCell>
            </TableRow>
          )}
          {pageEntries.map(({ row, displayIdx, payloadIdx }) => {
            const editable = payloadIdx !== null && payload !== null
            const ev = editable ? payload.events[payloadIdx] : null
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
                  <StatusBadge status={marked ? "removed" : row.status} />
                </TableCell>
                <TableCell>
                  {editable && ev && !marked ? (
                    <SelectCell
                      value={ev.event_type}
                      options={eventTypeOpts}
                      onChange={(v) => updatePayloadAt(payloadIdx, { event_type: v })}
                    />
                  ) : (
                    <span className="text-xs capitalize">{row.event_type}</span>
                  )}
                </TableCell>
                <TableCell>
                  {editable && ev && !marked ? (
                    <TextCell value={ev.symbol} onChange={(v) => updatePayloadAt(payloadIdx, { symbol: v })} />
                  ) : (
                    <span className="text-xs font-mono">{row.symbol}</span>
                  )}
                </TableCell>
                <TableCell>
                  {editable && ev && !marked ? (
                    <DateCell value={ev.date} onChange={(v) => updatePayloadAt(payloadIdx, { date: v })} />
                  ) : (
                    <span className="text-xs text-muted-foreground tabular-nums">{formatDate(row.date.split("T")[0] ?? row.date)}</span>
                  )}
                </TableCell>
                <TableCell className="text-right">
                  {editable && ev && !marked ? (
                    <DecimalCell value={ev.quantity} onChange={(v) => updatePayloadAt(payloadIdx, { quantity: v })} />
                  ) : (
                    <span className="text-xs tabular-nums">{row.quantity}</span>
                  )}
                </TableCell>
                <TableCell className="text-right">
                  {editable && ev && !marked ? (
                    <div className={cn(priceColorClass(ev.event_type))}>
                      <DecimalCell
                        value={ev.price_per_share}
                        onChange={(v) => updatePayloadAt(payloadIdx, { price_per_share: v })}
                      />
                    </div>
                  ) : (
                    <span className={cn("text-xs tabular-nums", priceColorClass(row.event_type))}>
                      {row.price_per_share}
                    </span>
                  )}
                </TableCell>
                <TableCell className="text-right">
                  {editable && ev && !marked ? (
                    <DecimalCell
                      value={ev.fee ?? ""}
                      onChange={(v) => updatePayloadAt(payloadIdx, { fee: v || null })}
                    />
                  ) : null}
                </TableCell>
                <TableCell>
                  {editable && ev && !marked ? (
                    <SelectCell
                      value={ev.currency}
                      options={currencyOpts}
                      onChange={(v) => updatePayloadAt(payloadIdx, { currency: v })}
                    />
                  ) : (
                    <span className="text-xs">{row.currency}</span>
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
