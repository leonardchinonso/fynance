import { useState } from "react"
import { api } from "@/api/client"
import type { IncomeCheck } from "@/bindings/IncomeCheck"
import type { IncomeCheckFigure } from "@/bindings/IncomeCheckFigure"
import type { IncomeCheckType } from "@/bindings/IncomeCheckType"
import type { IncomeLineType } from "@/bindings/IncomeLineType"
import type { IncomeSource } from "@/bindings/IncomeSource"
import { cn } from "@/lib/utils"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import {
  Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle,
} from "@/components/ui/dialog"
import { ConfirmDialog } from "@/components/confirm_dialog"
import { MONEY_RE, errorMessage } from "./tax_ui"

interface FigureField {
  line_type: IncomeLineType
  label: string
}

const FIELDS: Record<"p60" | "annual_accounts", FigureField[]> = {
  p60: [
    { line_type: "taxable_pay", label: "Pay in this employment" },
    { line_type: "paye_tax", label: "Tax deducted" },
  ],
  annual_accounts: [
    { line_type: "sales", label: "Turnover" },
    { line_type: "other_expenses", label: "Total expenses" },
  ],
}

export function checkTypeFor(source: IncomeSource): IncomeCheckType {
  return source.source_type === "employment" ? "p60" : "annual_accounts"
}

export function CheckDialog({
  source, check, taxYear, onClose, onSaved,
}: {
  source: IncomeSource
  check: IncomeCheck | undefined
  taxYear: string
  onClose: () => void
  onSaved: () => void
}) {
  const checkType = checkTypeFor(source) as "p60" | "annual_accounts"
  const fields = FIELDS[checkType]
  const [values, setValues] = useState<Record<string, string>>(() =>
    Object.fromEntries(fields.map((f) => [f.line_type, check?.figures.find((x) => x.line_type === f.line_type)?.amount ?? ""])),
  )
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [confirmDelete, setConfirmDelete] = useState(false)

  const valid = fields.every((f) => MONEY_RE.test((values[f.line_type] ?? "").trim()))

  async function handleSave() {
    if (!valid) return
    setSaving(true)
    setError(null)
    const figures: IncomeCheckFigure[] = fields.map((f) => ({
      line_type: f.line_type, amount: values[f.line_type].trim(), currency: "GBP",
    }))
    try {
      if (check) await api.updateIncomeCheck(check.id, { figures })
      else await api.createIncomeCheck({ source_id: source.id, tax_year: taxYear, check_type: checkType, figures, document_ids: [] })
      onSaved()
    } catch (err) {
      setError(errorMessage(err))
    } finally {
      setSaving(false)
    }
  }

  async function handleDelete() {
    if (!check) return
    setSaving(true)
    try {
      await api.deleteIncomeCheck(check.id)
      onSaved()
    } catch (err) {
      setError(errorMessage(err))
      setConfirmDelete(false)
    } finally {
      setSaving(false)
    }
  }

  const title = checkType === "p60" ? "P60" : "Annual accounts"

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{check ? "Edit" : "Add"} {title} for {source.name} ({taxYear})</DialogTitle>
        </DialogHeader>
        <div className="space-y-3 pt-2">
          <p className="text-xs text-muted-foreground">
            The printed figures are compared with the records and never added to them.
          </p>
          {fields.map((f) => (
            <div key={f.line_type}>
              <label className="text-sm font-medium">{f.label}</label>
              <Input
                inputMode="decimal"
                placeholder="0.00"
                value={values[f.line_type] ?? ""}
                onChange={(e) => setValues((v) => ({ ...v, [f.line_type]: e.target.value }))}
                className={cn((values[f.line_type] ?? "") !== "" && !MONEY_RE.test(values[f.line_type].trim()) && "border-destructive")}
              />
            </div>
          ))}
          {error && <p className="text-xs text-destructive">{error}</p>}
        </div>
        <DialogFooter>
          {check && (
            <Button variant="ghost" size="sm" className="mr-auto text-destructive" onClick={() => setConfirmDelete(true)} disabled={saving}>
              Delete
            </Button>
          )}
          <Button variant="outline" size="sm" onClick={onClose}>Cancel</Button>
          <Button size="sm" onClick={() => void handleSave()} disabled={!valid || saving}>
            {saving ? "Saving..." : check ? "Save" : "Create"}
          </Button>
        </DialogFooter>
        <ConfirmDialog
          open={confirmDelete}
          onOpenChange={setConfirmDelete}
          title={`Delete ${title}?`}
          busy={saving}
          error={error}
          onConfirm={() => void handleDelete()}
        >
          The records stay; only the printed figures are removed.
        </ConfirmDialog>
      </DialogContent>
    </Dialog>
  )
}
