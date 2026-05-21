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
import type { InvestmentIngestionResult } from "@/bindings/InvestmentIngestionResult"
import type { InvestmentsImportPayload } from "@/bindings/InvestmentsImportPayload"
import type { CreateInvestmentEventBody } from "@/bindings/CreateInvestmentEventBody"
import type { InvestmentEventType } from "@/bindings/InvestmentEventType"
import type { Currency } from "@/types"
import { StatusBadge } from "./status_badge"
import { DateCell, DecimalCell, SelectCell, TextCell } from "./editors"
import { SectionShell, useSectionControls } from "./section_shell"
import { Input } from "@/components/ui/input"
import {
  Select as UiSelect,
  SelectContent as UiSelectContent,
  SelectItem as UiSelectItem,
  SelectTrigger as UiSelectTrigger,
  SelectValue as UiSelectValue,
} from "@/components/ui/select"

const EVENT_TYPES: InvestmentEventType[] = [
  "buy", "sell", "vest", "transfer", "withhold", "split",
]

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
  const [dateFrom, setDateFrom] = useState("")
  const [dateTo, setDateTo] = useState("")
  const [eventFilter, setEventFilter] = useState("__all__")

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
    return result.rows
      .map((row, displayIdx) => ({ row, displayIdx, payloadIdx: rowPayloadIndex[displayIdx] }))
      .filter(({ row }) => {
        if (fromIso && row.date < fromIso) return false
        if (toIso && row.date > toIso) return false
        if (eventFilter !== "__all__" && row.event_type !== eventFilter) return false
        return true
      })
  }, [result.rows, rowPayloadIndex, dateFrom, dateTo, eventFilter])

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
        value={eventFilter}
        onValueChange={(v) => { if (v) { setEventFilter(v); ctrls.setPage(1) } }}
      >
        <UiSelectTrigger className="h-8 text-xs min-w-[8rem]">
          <UiSelectValue />
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
            <TableHead className="w-24">Action</TableHead>
            <TableHead className="w-28">Event</TableHead>
            <TableHead className="w-28">Symbol</TableHead>
            <TableHead className="w-36">Date</TableHead>
            <TableHead className="w-24 text-right">Qty</TableHead>
            <TableHead className="w-28 text-right">Price</TableHead>
            <TableHead className="w-24 text-right">Fee</TableHead>
            <TableHead className="w-20">Currency</TableHead>
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
                  <StatusBadge status={marked ? "duplicate" : row.status} />
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
                    <span className="text-xs">{(row.date.split("T")[0] ?? row.date)}</span>
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
