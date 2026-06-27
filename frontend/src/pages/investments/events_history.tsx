import { useMemo, useState } from "react"
import { api } from "@/api/client"
import type { Account } from "@/types"
import type { InvestmentEvent } from "@/bindings/InvestmentEvent"
import type { RemoteData } from "@/lib/remote_data"
import { visitRemoteData } from "@/lib/remote_data"
import { formatCurrency } from "@/lib/utils"
import { Button } from "@/components/ui/button"
import { Card, CardContent } from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"
import { EmptyState } from "@/components/empty_state"
import { AuthAwareError } from "@/components/auth_aware_error"
import { SettingsListSkeleton } from "@/components/skeletons"
import {
  Table, TableBody, TableCell, TableHead, TableHeader, TableRow,
} from "@/components/ui/table"
import { Select, SelectContent, SelectItem, SelectTrigger } from "@/components/ui/select"
import { Plus, Pencil, Trash2, TrendingUp } from "lucide-react"
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter } from "@/components/ui/dialog"
import { EventDialog, EVENT_TYPES } from "./event_dialog"

interface Props {
  data: RemoteData<InvestmentEvent[]>
  accounts: Account[]
  reload: () => void
  accountId?: string
  symbol?: string
  eventType?: string
  onFilterChange: (next: { accountId?: string; symbol?: string; eventType?: string }) => void
}

const ALL = "__all__"

export function EventsHistory({
  data, accounts, reload, accountId, symbol, eventType, onFilterChange,
}: Props) {
  const accountName = useMemo(() => {
    const map = new Map(accounts.map((a) => [a.id, a.name]))
    return (id: string) => map.get(id) ?? id
  }, [accounts])

  // Symbol options are derived from the loaded events so the filter only ever
  // offers symbols that actually exist.
  const symbolOptions = useMemo(() => {
    if (data.status !== "succeeded" && data.status !== "reloading") return []
    return Array.from(new Set(data.value.map((e) => e.symbol))).sort()
  }, [data])

  const [adding, setAdding] = useState(false)
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

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center gap-2">
        <Select
          value={accountId ?? ALL}
          onValueChange={(v) => onFilterChange({ accountId: pick(v), symbol, eventType })}
        >
          <SelectTrigger className="w-[180px]">
            <span className="truncate">{accountId ? accountName(accountId) : "All accounts"}</span>
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={ALL}>All accounts</SelectItem>
            {accounts.map((a) => <SelectItem key={a.id} value={a.id}>{a.name}</SelectItem>)}
          </SelectContent>
        </Select>

        <Select
          value={symbol ?? ALL}
          onValueChange={(v) => onFilterChange({ accountId, symbol: pick(v), eventType })}
        >
          <SelectTrigger className="w-[140px]">
            <span className="truncate">{symbol ?? "All symbols"}</span>
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={ALL}>All symbols</SelectItem>
            {symbolOptions.map((s) => <SelectItem key={s} value={s}>{s}</SelectItem>)}
          </SelectContent>
        </Select>

        <Select
          value={eventType ?? ALL}
          onValueChange={(v) => onFilterChange({ accountId, symbol, eventType: pick(v) })}
        >
          <SelectTrigger className="w-[130px]">
            <span className="truncate capitalize">{eventType ?? "All types"}</span>
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={ALL}>All types</SelectItem>
            {EVENT_TYPES.map((t) => <SelectItem key={t} value={t} className="capitalize">{t}</SelectItem>)}
          </SelectContent>
        </Select>

        <div className="flex-1" />
        <Button size="sm" className="gap-1.5" onClick={() => setAdding(true)} disabled={accounts.length === 0}>
          <Plus className="h-3.5 w-3.5" /> Add event
        </Button>
      </div>

      {visitRemoteData(data, {
        notLoaded: () => <SettingsListSkeleton rows={6} />,
        failed: (error) => <AuthAwareError error={error} onRetry={reload} />,
        hasValue: (events) =>
          events.length === 0 ? (
            <EmptyState
              icon={<TrendingUp className="h-8 w-8" />}
              title="No investment events"
              message="Add a buy, sell, or vest event, or adjust your filters to see more."
            />
          ) : (
            <Card>
              <CardContent className="p-0">
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>Date</TableHead>
                      <TableHead>Account</TableHead>
                      <TableHead>Symbol</TableHead>
                      <TableHead>Type</TableHead>
                      <TableHead className="text-right">Quantity</TableHead>
                      <TableHead className="text-right">Price/share</TableHead>
                      <TableHead className="text-right">Fee</TableHead>
                      <TableHead>Currency</TableHead>
                      <TableHead className="w-0" />
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {events.map((e) => (
                      <TableRow key={e.id} className="group">
                        <TableCell className="tabular-nums">{e.date.slice(0, 10)}</TableCell>
                        <TableCell className="text-muted-foreground">{accountName(e.account_id)}</TableCell>
                        <TableCell className="font-medium">{e.symbol}</TableCell>
                        <TableCell>
                          <Badge variant="secondary" className="capitalize font-normal">{e.event_type}</Badge>
                        </TableCell>
                        <TableCell className="text-right tabular-nums">{fmtQty(e.quantity)}</TableCell>
                        <TableCell className="text-right tabular-nums">{formatCurrency(e.price_per_share, e.currency)}</TableCell>
                        <TableCell className="text-right tabular-nums text-muted-foreground">
                          {e.fee ? formatCurrency(e.fee, e.fee_currency ?? e.currency) : "—"}
                        </TableCell>
                        <TableCell className="text-muted-foreground">{e.currency}</TableCell>
                        <TableCell className="text-right">
                          <div className="flex items-center justify-end gap-1">
                            <Button variant="ghost" size="icon" className="h-8 w-8 opacity-0 group-hover:opacity-100"
                              onClick={() => setEditing(e)} title="Edit event">
                              <Pencil className="h-3.5 w-3.5" />
                            </Button>
                            <Button variant="ghost" size="icon" className="h-8 w-8 opacity-0 group-hover:opacity-100"
                              onClick={() => setDeleting(e)} title="Delete event">
                              <Trash2 className="h-3.5 w-3.5" />
                            </Button>
                          </div>
                        </TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              </CardContent>
            </Card>
          ),
      })}

      {adding && (
        <EventDialog
          event={null}
          accounts={accounts}
          onClose={() => setAdding(false)}
          onSaved={() => { setAdding(false); reload() }}
        />
      )}

      {editing && (
        <EventDialog
          event={editing}
          accounts={accounts}
          onClose={() => setEditing(null)}
          onSaved={() => { setEditing(null); reload() }}
        />
      )}

      <Dialog open={!!deleting} onOpenChange={(open) => { if (!open) setDeleting(null) }}>
        <DialogContent className="sm:max-w-sm">
          <DialogHeader><DialogTitle>Delete investment event?</DialogTitle></DialogHeader>
          <p className="text-sm text-muted-foreground">
            This permanently removes the <strong>{deleting?.event_type}</strong> event for{" "}
            <strong>{deleting?.symbol}</strong> on {deleting?.date.slice(0, 10)}.
          </p>
          <DialogFooter>
            <Button variant="outline" size="sm" onClick={() => setDeleting(null)}>Cancel</Button>
            <Button variant="destructive" size="sm" onClick={handleDeleteConfirm}>Delete</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}

function fmtQty(qty: string): string {
  const n = Number.parseFloat(qty)
  if (!Number.isFinite(n)) return qty
  return n.toLocaleString("en-GB", { minimumFractionDigits: 0, maximumFractionDigits: 4 })
}

/** Map a Select value (which may be `null` or the "all" sentinel) to a filter
 * value: `undefined` clears the filter. */
function pick(v: string | null): string | undefined {
  return v && v !== ALL ? v : undefined
}
