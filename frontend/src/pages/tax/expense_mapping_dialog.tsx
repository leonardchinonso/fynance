import { useState } from "react"
import { api } from "@/api/client"
import type { IncomeLineType } from "@/bindings/IncomeLineType"
import type { IncomeSource } from "@/bindings/IncomeSource"
import type { LineTypeInfo } from "@/bindings/LineTypeInfo"
import type { SelfEmploymentSummary } from "@/bindings/SelfEmploymentSummary"
import { useCategoryOptions } from "@/hooks/data"
import { Button } from "@/components/ui/button"
import {
  Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle,
} from "@/components/ui/dialog"
import { Select, SelectContent, SelectItem, SelectTrigger } from "@/components/ui/select"
import { Money, errorMessage } from "./tax_ui"

const UNMAPPED = "unmapped"

/**
 * Pick the expense line each business-account category feeds. Categories
 * that are left unmapped are listed on the card and not counted.
 */
export function ExpenseMappingDialog({
  source, unmapped, catalogue, currency, onClose, onSaved,
}: {
  source: IncomeSource
  unmapped: SelfEmploymentSummary["bank_expenses"]["unmapped"]
  catalogue: LineTypeInfo[]
  currency: string
  onClose: () => void
  onSaved: () => void
}) {
  const categories = useCategoryOptions()
  const [map, setMap] = useState<Record<string, IncomeLineType | undefined>>({ ...source.expense_category_map })
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const targets = catalogue.filter((c) => c.source_type === "self_employment" && c.summed && c.line_type !== "sales" && c.line_type !== "refunds")
  const nameOf = (id: string) =>
    unmapped.find((u) => u.category_id === id)?.name ?? categories.find((c) => c.id === id)?.name ?? id
  const ids = [...new Set([...unmapped.map((u) => u.category_id), ...Object.keys(map)])]

  function choose(id: string, value: string) {
    setMap((m) => ({ ...m, [id]: value === UNMAPPED ? undefined : (value as IncomeLineType) }))
  }

  async function handleSave() {
    setSaving(true)
    setError(null)
    const cleaned = Object.fromEntries(Object.entries(map).filter(([, v]) => v !== undefined)) as Record<string, IncomeLineType>
    try {
      await api.updateIncomeSource(source.id, { expense_category_map: cleaned })
      onSaved()
    } catch (err) {
      setError(errorMessage(err))
    } finally {
      setSaving(false)
    }
  }

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="sm:max-w-lg max-h-[90vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>Expense categories for {source.name}</DialogTitle>
        </DialogHeader>
        <div className="space-y-3 pt-2">
          {ids.length === 0 && (
            <p className="text-sm text-muted-foreground">No outflows on the business accounts in this tax year.</p>
          )}
          {ids.map((id) => {
            const total = unmapped.find((u) => u.category_id === id)?.total
            const current = map[id]
            return (
              <div key={id} className="flex items-center gap-3 text-sm">
                <span className="min-w-0 flex-1 truncate">
                  {nameOf(id)}
                  {total && <span className="ml-1.5 text-xs text-muted-foreground"><Money amount={total} currency={currency} /></span>}
                </span>
                <Select value={current ?? UNMAPPED} onValueChange={(v) => v && choose(id, v)}>
                  <SelectTrigger className="w-[190px]">
                    <span className="truncate">{current ? catalogue.find((c) => c.line_type === current)?.label : "Not counted"}</span>
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value={UNMAPPED}>Not counted</SelectItem>
                    {targets.map((t) => <SelectItem key={t.line_type} value={t.line_type}>{t.label}</SelectItem>)}
                  </SelectContent>
                </Select>
              </div>
            )
          })}
          {error && <p className="text-xs text-destructive">{error}</p>}
        </div>
        <DialogFooter>
          <Button variant="outline" size="sm" onClick={onClose}>Cancel</Button>
          <Button size="sm" onClick={() => void handleSave()} disabled={saving}>{saving ? "Saving..." : "Save"}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
