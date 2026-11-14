import { useState } from "react"
import { api } from "@/api/client"
import { ukTaxYearToDates } from "@/api/cgt_filter_params"
import type { IncomeSource } from "@/bindings/IncomeSource"
import type { IncomeSyncResult } from "@/bindings/IncomeSyncResult"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import {
  Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle,
} from "@/components/ui/dialog"
import {
  Table, TableBody, TableCell, TableHead, TableHeader, TableRow,
} from "@/components/ui/table"
import { Money, errorMessage } from "./tax_ui"

/** The window to offer: the tax year, capped at today. */
function defaultWindow(taxYear: string): [string, string] {
  const [start, end] = ukTaxYearToDates(taxYear)
  const today = new Date().toISOString().slice(0, 10)
  return [start, end < today ? end : today > start ? today : end]
}

/**
 * Shop sync in two steps: a dry run shows what would change, then the user
 * confirms and the same request is sent again with `dry_run` off.
 */
export function SyncDialog({
  source, taxYear, currency, onClose, onSynced,
}: {
  source: IncomeSource
  taxYear: string
  currency: string
  onClose: () => void
  onSynced: () => void
}) {
  const [initialFrom, initialTo] = defaultWindow(taxYear)
  const [from, setFrom] = useState(initialFrom)
  const [to, setTo] = useState(initialTo)
  const [result, setResult] = useState<IncomeSyncResult | null>(null)
  const [applied, setApplied] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  async function run(dryRun: boolean) {
    setBusy(true)
    setError(null)
    try {
      const res = await api.importShopSync(source.id, { from, to, dry_run: dryRun })
      setResult(res)
      if (!dryRun) {
        setApplied(true)
        onSynced()
      }
    } catch (err) {
      setError(errorMessage(err))
    } finally {
      setBusy(false)
    }
  }

  const changes = result ? result.created + result.updated : 0

  return (
    <Dialog open onOpenChange={(open) => { if (!open && !busy) onClose() }}>
      <DialogContent className="sm:max-w-2xl max-h-[90vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>Sync {source.name} from the shop app</DialogTitle>
        </DialogHeader>
        <div className="space-y-3 pt-2">
          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="text-sm font-medium">From</label>
              <Input type="date" value={from} onChange={(e) => { setFrom(e.target.value); setResult(null) }} disabled={applied} />
            </div>
            <div>
              <label className="text-sm font-medium">To</label>
              <Input type="date" value={to} onChange={(e) => { setTo(e.target.value); setResult(null) }} disabled={applied} />
            </div>
          </div>

          {result && (
            <div className="space-y-3">
              <p className="text-sm">
                {applied ? "Synced: " : "Preview: "}
                {result.created} new, {result.updated} updated, {result.unchanged} unchanged.
              </p>
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>Period</TableHead>
                    <TableHead>Change</TableHead>
                    <TableHead className="text-right">Sales</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {result.periods.map((p) => {
                    const sales = p.line_items.find((i) => i.line_type === "sales")
                    return (
                      <TableRow key={p.external_ref}>
                        <TableCell className="whitespace-nowrap">{p.external_ref.replace("..", " to ")}</TableCell>
                        <TableCell>
                          <Badge variant={p.status === "unchanged" ? "outline" : "secondary"}>{p.status}</Badge>
                        </TableCell>
                        <TableCell className="text-right">
                          {sales ? <Money amount={sales.amount} currency={currency} /> : "-"}
                        </TableCell>
                      </TableRow>
                    )
                  })}
                </TableBody>
              </Table>
              {result.late_refunds.length > 0 && (
                <div className="rounded-md border px-3 py-2 text-sm">
                  <p className="font-medium">Refunds issued in a later month than their order</p>
                  <ul className="mt-1 space-y-0.5 text-muted-foreground">
                    {result.late_refunds.map((r) => (
                      <li key={`${r.order_month}-${r.refund_date}`}>
                        Order of {r.order_month}, refunded {r.refund_date}: <Money amount={r.amount} currency={currency} />
                      </li>
                    ))}
                  </ul>
                  <p className="mt-1 text-xs text-muted-foreground">
                    Sales are counted by order date, so these refunds fall in the month they were issued.
                  </p>
                </div>
              )}
              {result.warnings.map((w) => (
                <p key={w} className="text-xs text-muted-foreground">{w}</p>
              ))}
            </div>
          )}
          {error && <p className="text-xs text-destructive">{error}</p>}
        </div>
        <DialogFooter>
          <Button variant="outline" size="sm" onClick={onClose} disabled={busy}>{applied ? "Close" : "Cancel"}</Button>
          {!applied && (
            <Button variant="outline" size="sm" onClick={() => void run(true)} disabled={busy || !from || !to || to < from}>
              {busy && !result ? "Checking..." : "Preview changes"}
            </Button>
          )}
          {!applied && result && (
            <Button size="sm" onClick={() => void run(false)} disabled={busy || changes === 0}>
              {busy ? "Syncing..." : `Apply ${changes} change${changes === 1 ? "" : "s"}`}
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
