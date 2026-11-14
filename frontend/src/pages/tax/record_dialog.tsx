import { useMemo, useState } from "react"
import { api } from "@/api/client"
import type { IncomeBasis } from "@/bindings/IncomeBasis"
import type { IncomeLineItemInput } from "@/bindings/IncomeLineItemInput"
import type { IncomeLineType } from "@/bindings/IncomeLineType"
import type { IncomeRecord } from "@/bindings/IncomeRecord"
import type { IncomeSource } from "@/bindings/IncomeSource"
import type { LineTypeInfo } from "@/bindings/LineTypeInfo"
import { cn } from "@/lib/utils"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Textarea } from "@/components/ui/textarea"
import {
  Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle,
} from "@/components/ui/dialog"
import { Select, SelectContent, SelectItem, SelectTrigger } from "@/components/ui/select"
import { Plus, Trash2 } from "lucide-react"
import { ukTaxYearToDates } from "@/api/cgt_filter_params"
import { lineItemToInput } from "./tax_helpers"
import { MONEY_RE, SIGNED_MONEY_RE, errorMessage } from "./tax_ui"

interface LineDraft {
  line_type: IncomeLineType
  amount: string
  basis: IncomeBasis
  /** Carried through unchanged so an edit keeps the printed label and RSU link. */
  label?: string
  investment_event_id?: string
}

const DEFAULT_LINES: Record<string, IncomeLineType[]> = {
  employment: ["taxable_pay", "paye_tax"],
  self_employment: ["sales"],
}

function initialLines(source: IncomeSource, record: IncomeRecord | null): LineDraft[] {
  if (record) {
    return record.line_items.map((i) => ({ ...lineItemToInput(i), basis: i.basis }))
  }
  return (DEFAULT_LINES[source.source_type] ?? []).map((line_type) => ({
    line_type, amount: "", basis: "period" as const,
  }))
}

function toInput(d: LineDraft, currency: string): IncomeLineItemInput {
  return {
    line_type: d.line_type, amount: d.amount.trim(), currency, basis: d.basis,
    label: d.label, investment_event_id: d.investment_event_id,
  }
}

export function RecordDialog({
  source, record, taxYear, catalogue, onClose, onSaved,
}: {
  source: IncomeSource
  record: IncomeRecord | null
  taxYear: string
  catalogue: LineTypeInfo[]
  onClose: () => void
  onSaved: () => void
}) {
  const isEdit = record !== null
  const isAdjustment = record?.kind === "year_end_adjustment"
  const employment = source.source_type === "employment"
  const [yearStart] = useMemo(() => ukTaxYearToDates(taxYear), [taxYear])
  const [periodStart, setPeriodStart] = useState(record?.period_start ?? yearStart)
  const [periodEnd, setPeriodEnd] = useState(record?.period_end ?? yearStart)
  const [payDate, setPayDate] = useState(record?.pay_date ?? "")
  const [currency, setCurrency] = useState(record?.currency ?? "GBP")
  const [notes, setNotes] = useState(record?.notes ?? "")
  const [lines, setLines] = useState<LineDraft[]>(() => initialLines(source, record))
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const options = catalogue.filter((c) => c.source_type === source.source_type)
  const amountRe = isAdjustment ? SIGNED_MONEY_RE : MONEY_RE
  const amountOk = (v: string) => amountRe.test(v.trim())
  const valid =
    periodStart !== "" && periodEnd !== "" && periodEnd >= periodStart &&
    currency.trim() !== "" && lines.length > 0 && lines.every((l) => amountOk(l.amount))

  function updateLine(index: number, patch: Partial<LineDraft>) {
    setLines((ls) => ls.map((l, i) => (i === index ? { ...l, ...patch } : l)))
  }

  async function handleSave() {
    if (!valid) {
      setError("Check the dates and amounts: amounts must be non-negative decimals and at least one line is needed.")
      return
    }
    setSaving(true)
    setError(null)
    const code = currency.trim().toUpperCase()
    const items = lines.map((l) => toInput(l, code))
    try {
      if (record) {
        await api.updateIncomeRecord(record.id, {
          period_start: periodStart,
          period_end: periodEnd,
          pay_date: payDate === "" ? null : payDate,
          currency: code,
          notes: notes.trim() === "" ? null : notes.trim(),
          line_items: items,
        })
      } else {
        await api.createIncomeRecord({
          source_id: source.id,
          period_start: periodStart,
          period_end: periodEnd,
          pay_date: payDate === "" ? undefined : payDate,
          currency: code,
          transaction_ids: [],
          document_ids: [],
          notes: notes.trim() === "" ? undefined : notes.trim(),
          line_items: items,
        })
      }
      onSaved()
    } catch (err) {
      setError(errorMessage(err))
    } finally {
      setSaving(false)
    }
  }

  const title = `${isEdit ? "Edit" : "Add"} ${employment ? "payslip" : "month"} for ${source.name}`

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="sm:max-w-xl max-h-[90vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
        </DialogHeader>
        <div className="space-y-3 pt-2">
          <div className={cn("grid gap-3", employment ? "grid-cols-3" : "grid-cols-2")}>
            <div>
              <label className="text-sm font-medium">Period start</label>
              <Input type="date" value={periodStart} onChange={(e) => setPeriodStart(e.target.value)} />
            </div>
            <div>
              <label className="text-sm font-medium">Period end</label>
              <Input
                type="date" value={periodEnd} onChange={(e) => setPeriodEnd(e.target.value)}
                className={cn(periodEnd < periodStart && "border-destructive")}
              />
            </div>
            {employment && (
              <div>
                <label className="text-sm font-medium">Pay date</label>
                <Input type="date" value={payDate} onChange={(e) => setPayDate(e.target.value)} />
              </div>
            )}
          </div>

          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <label className="text-sm font-medium">Line items</label>
              <div className="w-24">
                <Input
                  aria-label="Currency"
                  value={currency}
                  onChange={(e) => setCurrency(e.target.value.toUpperCase())}
                  className="h-8"
                />
              </div>
            </div>
            {lines.map((l, i) => (
              <div key={i} className="flex items-center gap-2">
                <Select value={l.line_type} onValueChange={(v) => v && updateLine(i, { line_type: v as IncomeLineType })}>
                  <SelectTrigger className="w-[190px]">
                    <span className="truncate">{options.find((o) => o.line_type === l.line_type)?.label ?? l.line_type}</span>
                  </SelectTrigger>
                  <SelectContent>
                    {options.map((o) => <SelectItem key={o.line_type} value={o.line_type}>{o.label}</SelectItem>)}
                  </SelectContent>
                </Select>
                <Input
                  inputMode="decimal"
                  placeholder="0.00"
                  value={l.amount}
                  onChange={(e) => updateLine(i, { amount: e.target.value })}
                  className={cn("flex-1", l.amount !== "" && !amountOk(l.amount) && "border-destructive")}
                />
                <Select value={l.basis} onValueChange={(v) => v && updateLine(i, { basis: v as IncomeBasis })}>
                  <SelectTrigger className="w-[120px]">
                    <span>{l.basis === "period" ? "Period" : "Year to date"}</span>
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="period">Period</SelectItem>
                    <SelectItem value="year_to_date">Year to date</SelectItem>
                  </SelectContent>
                </Select>
                <Button
                  variant="ghost" size="icon" className="h-8 w-8" title="Remove line"
                  onClick={() => setLines((ls) => ls.filter((_, n) => n !== i))}
                >
                  <Trash2 className="h-3.5 w-3.5 text-muted-foreground" />
                </Button>
              </div>
            ))}
            <Button
              variant="outline" size="sm"
              onClick={() => setLines((ls) => [...ls, { line_type: options[0]?.line_type ?? "taxable_pay", amount: "", basis: "period" }])}
            >
              <Plus className="mr-1.5 h-3.5 w-3.5" /> Add line
            </Button>
            <p className="text-xs text-muted-foreground">
              Year-to-date lines are kept for reference and never added to the totals.
            </p>
          </div>

          <div>
            <label className="text-sm font-medium">Notes (optional)</label>
            <Textarea value={notes} onChange={(e) => setNotes(e.target.value)} />
          </div>
          {error && <p className="text-xs text-destructive">{error}</p>}
        </div>
        <DialogFooter>
          <Button variant="outline" size="sm" onClick={onClose}>Cancel</Button>
          <Button size="sm" onClick={() => void handleSave()} disabled={!valid || saving}>
            {saving ? "Saving..." : isEdit ? "Save" : "Create"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
