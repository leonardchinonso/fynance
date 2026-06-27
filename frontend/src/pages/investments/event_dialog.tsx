import { useState } from "react"
import { api } from "@/api/client"
import type { Account } from "@/types"
import type { InvestmentEvent } from "@/bindings/InvestmentEvent"
import type { InvestmentEventType } from "@/bindings/InvestmentEventType"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter } from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { Textarea } from "@/components/ui/textarea"
import { Select, SelectContent, SelectItem, SelectTrigger } from "@/components/ui/select"

export const EVENT_TYPES: InvestmentEventType[] = [
  "vest", "buy", "sell", "transfer", "withhold", "split",
]

interface EventFormState {
  accountId: string
  eventType: InvestmentEventType
  symbol: string
  date: string
  quantity: string
  pricePerShare: string
  fee: string
  currency: string
  feeCurrency: string
  notes: string
}

/** "YYYY-MM-DDTHH:MM:SS" or "YYYY-MM-DD" → "YYYY-MM-DD" for the date input. */
function toDateInput(iso: string): string {
  return iso.slice(0, 10)
}

/** "YYYY-MM-DD" → "YYYY-MM-DDT00:00:00" wire format. */
function toWireDate(date: string): string {
  return date.includes("T") ? date : `${date}T00:00:00`
}

function emptyForm(accounts: Account[]): EventFormState {
  const firstInvestment = accounts.find((a) => a.type === "investment" || a.type === "investment_isa")
  return {
    accountId: firstInvestment?.id ?? accounts[0]?.id ?? "",
    eventType: "buy",
    symbol: "",
    date: new Date().toISOString().slice(0, 10),
    quantity: "",
    pricePerShare: "",
    fee: "",
    currency: firstInvestment?.currency ?? accounts[0]?.currency ?? "GBP",
    feeCurrency: "",
    notes: "",
  }
}

function formFromEvent(event: InvestmentEvent): EventFormState {
  return {
    accountId: event.account_id,
    eventType: event.event_type,
    symbol: event.symbol,
    date: toDateInput(event.date),
    quantity: event.quantity,
    pricePerShare: event.price_per_share,
    fee: event.fee ?? "",
    currency: event.currency,
    feeCurrency: event.fee_currency ?? "",
    notes: event.notes ?? "",
  }
}

interface Props {
  /** When set, the dialog edits this event; otherwise it creates a new one. */
  event: InvestmentEvent | null
  accounts: Account[]
  onClose: () => void
  onSaved: () => void
}

/**
 * Add/Edit dialog for a single investment event. Money fields stay as strings
 * end-to-end (decimal-as-string); we never parse them to float for storage.
 */
export function EventDialog({ event, accounts, onClose, onSaved }: Props) {
  const isEdit = event !== null
  const [form, setForm] = useState<EventFormState>(
    () => (event ? formFromEvent(event) : emptyForm(accounts)),
  )
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const valid =
    form.accountId.trim() !== "" &&
    form.symbol.trim() !== "" &&
    form.date.trim() !== "" &&
    form.quantity.trim() !== "" &&
    form.pricePerShare.trim() !== "" &&
    form.currency.trim() !== ""

  function update<K extends keyof EventFormState>(key: K, value: EventFormState[K]) {
    setForm((f) => ({ ...f, [key]: value }))
  }

  async function handleSave() {
    if (!valid) return
    setSaving(true)
    setError(null)
    try {
      const fee = form.fee.trim() === "" ? null : form.fee.trim()
      const feeCurrency = form.feeCurrency.trim() === "" ? null : form.feeCurrency.trim().toUpperCase()
      const notes = form.notes.trim() === "" ? null : form.notes.trim()
      if (isEdit && event) {
        await api.updateInvestment(event.id, {
          event_type: form.eventType,
          symbol: form.symbol.trim().toUpperCase(),
          date: toWireDate(form.date),
          quantity: form.quantity.trim(),
          price_per_share: form.pricePerShare.trim(),
          fee,
          currency: form.currency.trim().toUpperCase(),
          fee_currency: feeCurrency,
          notes,
        })
      } else {
        await api.createInvestment({
          account_id: form.accountId,
          event_type: form.eventType,
          symbol: form.symbol.trim().toUpperCase(),
          date: toWireDate(form.date),
          quantity: form.quantity.trim(),
          price_per_share: form.pricePerShare.trim(),
          fee,
          currency: form.currency.trim().toUpperCase(),
          fee_currency: feeCurrency,
          notes,
          source_document_ids: [],
        })
      }
      onSaved()
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setSaving(false)
    }
  }

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{isEdit ? "Edit investment event" : "Add investment event"}</DialogTitle>
        </DialogHeader>
        <div className="space-y-3 pt-2">
          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="text-sm font-medium">Account</label>
              <Select value={form.accountId} onValueChange={(v) => update("accountId", v ?? "")} disabled={isEdit}>
                <SelectTrigger className="w-full">
                  <span className="truncate">{accounts.find((a) => a.id === form.accountId)?.name ?? "Select account"}</span>
                </SelectTrigger>
                <SelectContent>
                  {accounts.map((a) => <SelectItem key={a.id} value={a.id}>{a.name}</SelectItem>)}
                </SelectContent>
              </Select>
            </div>
            <div>
              <label className="text-sm font-medium">Type</label>
              <Select value={form.eventType} onValueChange={(v) => update("eventType", v as InvestmentEventType)}>
                <SelectTrigger className="w-full">
                  <span className="capitalize">{form.eventType}</span>
                </SelectTrigger>
                <SelectContent>
                  {EVENT_TYPES.map((t) => <SelectItem key={t} value={t} className="capitalize">{t}</SelectItem>)}
                </SelectContent>
              </Select>
            </div>
          </div>
          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="text-sm font-medium">Symbol</label>
              <Input placeholder="e.g. VUSA" value={form.symbol}
                onChange={(e) => update("symbol", e.target.value.toUpperCase())} autoFocus={!isEdit} />
            </div>
            <div>
              <label className="text-sm font-medium">Date</label>
              <Input type="date" value={form.date} onChange={(e) => update("date", e.target.value)} />
            </div>
          </div>
          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="text-sm font-medium">Quantity</label>
              <Input inputMode="decimal" placeholder="e.g. 10" value={form.quantity}
                onChange={(e) => update("quantity", e.target.value)} />
            </div>
            <div>
              <label className="text-sm font-medium">Price/share</label>
              <Input inputMode="decimal" placeholder="e.g. 72.00" value={form.pricePerShare}
                onChange={(e) => update("pricePerShare", e.target.value)} />
            </div>
          </div>
          <div className="grid grid-cols-3 gap-3">
            <div>
              <label className="text-sm font-medium">Fee</label>
              <Input inputMode="decimal" placeholder="0.00" value={form.fee}
                onChange={(e) => update("fee", e.target.value)} />
            </div>
            <div>
              <label className="text-sm font-medium">Currency</label>
              <Input placeholder="GBP" value={form.currency}
                onChange={(e) => update("currency", e.target.value.toUpperCase())} />
            </div>
            <div>
              <label className="text-sm font-medium">Fee currency</label>
              <Input placeholder="optional" value={form.feeCurrency}
                onChange={(e) => update("feeCurrency", e.target.value.toUpperCase())} />
            </div>
          </div>
          <div>
            <label className="text-sm font-medium">Notes (optional)</label>
            <Textarea placeholder="Any additional notes" value={form.notes}
              onChange={(e) => update("notes", e.target.value)} />
          </div>
          {error && <p className="text-xs text-destructive">{error}</p>}
        </div>
        <DialogFooter>
          <Button variant="outline" size="sm" onClick={onClose}>Cancel</Button>
          <Button size="sm" onClick={handleSave} disabled={!valid || saving}>
            {saving ? "Saving..." : isEdit ? "Save" : "Create"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
