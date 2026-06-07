import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { formatCurrency } from "@/lib/utils"
import { cn } from "@/lib/utils"
import type { CgtRealizedEvent } from "@/bindings/CgtRealizedEvent"

const UNMATCHED_TITLE =
  "No matching acquisition was found in the same-day, 30-day, or S104 pool windows. The full proceeds are recorded as a gain and the cost basis is zero."

interface CgtDisposalScheduleProps {
  rows: CgtRealizedEvent[]
}

export function CgtDisposalSchedule({ rows }: CgtDisposalScheduleProps) {
  if (rows.length === 0) {
    return (
      <Card>
        <CardHeader>
          <CardTitle>Disposal schedule</CardTitle>
        </CardHeader>
        <CardContent>
          <p className="text-sm text-muted-foreground">
            No realised disposals in this period. Your S104 pool state is shown below.
          </p>
        </CardContent>
      </Card>
    )
  }

  const sorted = [...rows].sort((a, b) => a.disposal_date.localeCompare(b.disposal_date))

  return (
    <Card>
      <CardHeader>
        <CardTitle>Disposal schedule</CardTitle>
      </CardHeader>
      <CardContent>
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Symbol</TableHead>
              <TableHead>Disposal date</TableHead>
              <TableHead>Acquisition date</TableHead>
              <TableHead className="text-right">Qty</TableHead>
              <TableHead className="text-right">Disposal price</TableHead>
              <TableHead className="text-right">Proceeds</TableHead>
              <TableHead className="text-right">Cost basis</TableHead>
              <TableHead className="text-right">Gain/(Loss)</TableHead>
              <TableHead>Rule</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {sorted.map((e, idx) => (
              <DisposalRow key={`${e.disposal_id}-${idx}`} event={e} />
            ))}
          </TableBody>
        </Table>
      </CardContent>
    </Card>
  )
}

function DisposalRow({ event }: { event: CgtRealizedEvent }) {
  const gain = Number.parseFloat(event.gain_loss)
  const unmatched = event.rule_applied === "Unmatched"
  const acquisitionLabel = formatAcquisition(event)
  return (
    <TableRow
      className={cn(unmatched && "bg-amber-50 dark:bg-amber-900/20")}
      title={unmatched ? UNMATCHED_TITLE : undefined}
    >
      <TableCell className="font-medium">
        <div className="flex items-center gap-2">
          {event.symbol}
          <span className="text-[10px] text-muted-foreground">{event.original_currency}</span>
        </div>
      </TableCell>
      <TableCell className="tabular-nums">{event.disposal_date.slice(0, 10)}</TableCell>
      <TableCell className="tabular-nums">{acquisitionLabel}</TableCell>
      <TableCell className="text-right tabular-nums">{event.quantity}</TableCell>
      <TableCell className="text-right tabular-nums">
        {formatCurrency(event.disposal_price, event.original_currency)}
      </TableCell>
      <TableCell className="text-right tabular-nums">
        {formatCurrency(event.proceeds, event.original_currency)}
      </TableCell>
      <TableCell className="text-right tabular-nums">
        {formatCurrency(event.cost_basis, event.original_currency)}
      </TableCell>
      <TableCell
        className={
          "text-right tabular-nums font-medium " +
          (gain >= 0
            ? "text-emerald-600 dark:text-emerald-400"
            : "text-red-600 dark:text-red-400")
        }
      >
        {formatCurrency(event.gain_loss, event.original_currency)}
      </TableCell>
      <TableCell>
        <RulePill rule={event.rule_applied} />
      </TableCell>
    </TableRow>
  )
}

function formatAcquisition(event: CgtRealizedEvent): string {
  const first = event.matches[0]
  if (!first) return "—"
  if (first.acquisition_date === null) return "—"
  if (first.acquisition_date === "S104 Pool") return "S104 Pool"
  return first.acquisition_date.slice(0, 10)
}

function RulePill({ rule }: { rule: string }) {
  const styles: Record<string, string> = {
    "Same-Day": "bg-blue-100 text-blue-900 dark:bg-blue-900/40 dark:text-blue-200",
    "30-Day Rule": "bg-violet-100 text-violet-900 dark:bg-violet-900/40 dark:text-violet-200",
    "S104 Pool": "bg-emerald-100 text-emerald-900 dark:bg-emerald-900/40 dark:text-emerald-200",
    Unmatched: "bg-amber-100 text-amber-900 dark:bg-amber-900/40 dark:text-amber-200",
  }
  const className = styles[rule] ?? "bg-muted text-foreground"
  return (
    <span
      className={cn(
        "inline-flex items-center rounded-full px-2 py-0.5 text-[11px] font-medium",
        className,
      )}
    >
      {rule}
    </span>
  )
}
