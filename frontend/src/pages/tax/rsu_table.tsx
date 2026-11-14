import { useState } from "react"
import { api } from "@/api/client"
import type { IncomeRecord } from "@/bindings/IncomeRecord"
import type { RsuVestSummary } from "@/bindings/RsuVestSummary"
import type { VestCandidate } from "@/bindings/VestCandidate"
import { useVestCandidates } from "@/hooks/data"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Select, SelectContent, SelectItem, SelectTrigger } from "@/components/ui/select"
import {
  Table, TableBody, TableCell, TableHead, TableHeader, TableRow,
} from "@/components/ui/table"
import { ConfirmDialog } from "@/components/confirm_dialog"
import { alignedPricePerShare, lineItemToInput, recordMonth } from "./tax_helpers"
import { Money, errorMessage } from "./tax_ui"

const NO_LINK = "none"

function MatchBadge({ status }: { status: RsuVestSummary["status"] }) {
  if (status === "matched") {
    return (
      <Badge variant="secondary" className="bg-emerald-500/15 text-emerald-700 dark:text-emerald-400">
        Matched
      </Badge>
    )
  }
  if (status === "mismatch") return <Badge variant="destructive">Mismatch</Badge>
  return <Badge variant="outline">Not linked</Badge>
}

function candidateLabel(c: VestCandidate): string {
  return `${c.symbol} ${c.date}, ${c.quantity} sh (${c.account_name})`
}

function RsuRow({
  rsu, record, currency, onChanged,
}: {
  rsu: RsuVestSummary
  record: IncomeRecord | undefined
  currency: string
  onChanged: () => void
}) {
  const [candidates] = useVestCandidates(rsu.record_id)
  const [aligning, setAligning] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const list = candidates.status === "succeeded" || candidates.status === "reloading" ? candidates.value : []
  const linked = list.find((c) => c.investment_event_id === rsu.investment_event_id)
  const newPrice = linked ? alignedPricePerShare(linked.price_per_share, linked.value_gbp, rsu.vest_value) : null

  async function link(eventId: string | undefined) {
    if (!record) return
    setBusy(true)
    setError(null)
    try {
      await api.updateIncomeRecord(record.id, {
        line_items: record.line_items.map((i) => ({
          ...lineItemToInput(i),
          investment_event_id: i.id === rsu.line_item_id ? eventId : i.investment_event_id,
        })),
      })
      onChanged()
    } catch (err) {
      setError(errorMessage(err))
    } finally {
      setBusy(false)
    }
  }

  async function align() {
    if (!linked || !newPrice) return
    setBusy(true)
    setError(null)
    try {
      await api.updateInvestment(linked.investment_event_id, {
        event_type: null, symbol: null, date: null, quantity: null, price_per_share: newPrice,
        fee: null, currency: null, fee_currency: null, notes: null,
      })
      setAligning(false)
      onChanged()
    } catch (err) {
      setError(errorMessage(err))
    } finally {
      setBusy(false)
    }
  }

  return (
    <TableRow>
      <TableCell className="whitespace-nowrap">{record ? recordMonth(record) : "-"}</TableCell>
      <TableCell className="text-right"><Money amount={rsu.vest_value} currency={currency} /></TableCell>
      <TableCell>
        <Select
          value={rsu.investment_event_id ?? NO_LINK}
          onValueChange={(v) => v && void link(v === NO_LINK ? undefined : v)}
          disabled={busy}
        >
          <SelectTrigger className="w-[260px] max-w-full">
            <span className="truncate">
              {linked ? candidateLabel(linked) : rsu.investment_event_id ? "Linked vest event" : "Link a vest event"}
            </span>
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={NO_LINK}>No link</SelectItem>
            {list.map((c) => (
              <SelectItem key={c.investment_event_id} value={c.investment_event_id}>
                {candidateLabel(c)}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        {error && <p className="mt-1 text-xs text-destructive">{error}</p>}
      </TableCell>
      <TableCell className="text-right">
        {rsu.event_value ? <Money amount={rsu.event_value} currency={currency} /> : <span className="text-muted-foreground">-</span>}
      </TableCell>
      <TableCell><MatchBadge status={rsu.status} /></TableCell>
      <TableCell className="text-right">
        {rsu.status === "mismatch" && (
          <Button
            variant="outline"
            size="sm"
            disabled={!newPrice || busy}
            title={newPrice ? undefined : "The vest event is not among the nearby candidates"}
            onClick={() => { setError(null); setAligning(true) }}
          >
            Align cost basis
          </Button>
        )}
      </TableCell>
      <ConfirmDialog
        open={aligning}
        onOpenChange={setAligning}
        title="Align cost basis?"
        confirmLabel="Align"
        busyLabel="Aligning..."
        busy={busy}
        error={error}
        onConfirm={() => void align()}
      >
        Sets the price per share of the {linked?.symbol} vest on {linked?.date} from{" "}
        {linked?.price_per_share} to {newPrice} {linked?.currency}, so its value matches the{" "}
        <Money amount={rsu.vest_value} currency={currency} /> taxed as pay. Capital gains figures that use this event will change.
      </ConfirmDialog>
    </TableRow>
  )
}

export function RsuTable({
  rsu, records, currency, onChanged,
}: {
  rsu: RsuVestSummary[]
  records: IncomeRecord[]
  currency: string
  onChanged: () => void
}) {
  if (rsu.length === 0) return null
  return (
    <div className="space-y-2">
      <h3 className="text-sm font-medium">RSU vests</h3>
      <div className="overflow-x-auto">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Month</TableHead>
              <TableHead className="text-right">Taxed as pay</TableHead>
              <TableHead>Vest event</TableHead>
              <TableHead className="text-right">Event value</TableHead>
              <TableHead>Status</TableHead>
              <TableHead />
            </TableRow>
          </TableHeader>
          <TableBody>
            {rsu.map((r) => (
              <RsuRow
                key={r.line_item_id}
                rsu={r}
                record={records.find((rec) => rec.id === r.record_id)}
                currency={currency}
                onChanged={onChanged}
              />
            ))}
          </TableBody>
        </Table>
      </div>
    </div>
  )
}
