import { useState } from "react"
import { api } from "@/api/client"
import type { IncomeSource } from "@/bindings/IncomeSource"
import type { IncomeSourceIdentifiers } from "@/bindings/IncomeSourceIdentifiers"
import type { IncomeSourceType } from "@/bindings/IncomeSourceType"
import { useAccounts } from "@/hooks/data"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Switch } from "@/components/ui/switch"
import {
  Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle,
} from "@/components/ui/dialog"
import { Select, SelectContent, SelectItem, SelectTrigger } from "@/components/ui/select"
import { ConfirmDialog } from "@/components/confirm_dialog"
import { errorMessage } from "./tax_ui"

const TYPE_LABELS: Partial<Record<IncomeSourceType, string>> = {
  employment: "Employment",
  self_employment: "Self-employment",
}

export function SourceDialog({
  profileId, source, onClose, onSaved,
}: {
  profileId: string
  source: IncomeSource | null
  onClose: () => void
  onSaved: () => void
}) {
  const isEdit = source !== null
  const [sourceType, setSourceType] = useState<IncomeSourceType>(source?.source_type ?? "employment")
  const [name, setName] = useState(source?.name ?? "")
  const [payeRef, setPayeRef] = useState(source?.identifiers.paye_reference ?? "")
  const [description, setDescription] = useState(source?.identifiers.description ?? "")
  const [accountIds, setAccountIds] = useState<string[]>(source?.account_ids ?? [])
  const [shopSync, setShopSync] = useState(source?.sync_kind === "shop_app")
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [confirmDelete, setConfirmDelete] = useState(false)
  const [accountsData] = useAccounts(profileId)
  const accounts = accountsData.status === "succeeded" || accountsData.status === "reloading" ? accountsData.value : []

  const business = sourceType === "self_employment"
  const valid = name.trim() !== ""

  function identifiers(): IncomeSourceIdentifiers {
    if (business) {
      return {
        description: description.trim() === "" ? undefined : description.trim(),
        accounting_basis: "cash",
      }
    }
    return { ...source?.identifiers, paye_reference: payeRef.trim() === "" ? undefined : payeRef.trim() }
  }

  async function handleSave() {
    if (!valid) return
    setSaving(true)
    setError(null)
    try {
      if (source) {
        await api.updateIncomeSource(source.id, {
          name: name.trim(),
          identifiers: identifiers(),
          account_ids: business ? accountIds : undefined,
          sync_kind: business ? (shopSync ? "shop_app" : null) : undefined,
        })
      } else {
        await api.createIncomeSource({
          profile_id: profileId,
          source_type: sourceType,
          name: name.trim(),
          identifiers: identifiers(),
          account_ids: business ? accountIds : [],
          expense_category_map: {},
          sync_kind: business && shopSync ? "shop_app" : undefined,
          active: true,
        })
      }
      onSaved()
    } catch (err) {
      setError(errorMessage(err))
    } finally {
      setSaving(false)
    }
  }

  async function handleDelete() {
    if (!source) return
    setSaving(true)
    setError(null)
    try {
      await api.deleteIncomeSource(source.id)
      onSaved()
    } catch (err) {
      setError(errorMessage(err))
      setConfirmDelete(false)
    } finally {
      setSaving(false)
    }
  }

  function toggleAccount(id: string) {
    setAccountIds((ids) => (ids.includes(id) ? ids.filter((x) => x !== id) : [...ids, id]))
  }

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="sm:max-w-lg max-h-[90vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{isEdit ? "Edit income source" : "Add income source"}</DialogTitle>
        </DialogHeader>
        <div className="space-y-3 pt-2">
          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="text-sm font-medium">Type</label>
              <Select value={sourceType} onValueChange={(v) => v && setSourceType(v as IncomeSourceType)} disabled={isEdit}>
                <SelectTrigger className="w-full"><span>{TYPE_LABELS[sourceType]}</span></SelectTrigger>
                <SelectContent>
                  {Object.entries(TYPE_LABELS).map(([value, label]) => (
                    <SelectItem key={value} value={value}>{label}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div>
              <label className="text-sm font-medium">{business ? "Business name" : "Employer"}</label>
              <Input value={name} onChange={(e) => setName(e.target.value)} placeholder={business ? "e.g. Example Craft Shop" : "e.g. Employer A"} autoFocus={!isEdit} />
            </div>
          </div>

          {!business && (
            <div>
              <label className="text-sm font-medium">PAYE reference (optional)</label>
              <Input value={payeRef} onChange={(e) => setPayeRef(e.target.value)} placeholder="123/AB456" />
            </div>
          )}

          {business && (
            <>
              <div>
                <label className="text-sm font-medium">Description (optional)</label>
                <Input value={description} onChange={(e) => setDescription(e.target.value)} />
              </div>
              <div className="space-y-1.5">
                <label className="text-sm font-medium">Business bank accounts</label>
                <div className="max-h-40 space-y-1 overflow-y-auto rounded-md border p-2">
                  {accounts.length === 0 && <p className="text-xs text-muted-foreground">No accounts for this profile.</p>}
                  {accounts.map((a) => (
                    <label key={a.id} className="flex items-center gap-2 text-sm">
                      <input type="checkbox" checked={accountIds.includes(a.id)} onChange={() => toggleAccount(a.id)} />
                      {a.name}
                    </label>
                  ))}
                </div>
                <p className="text-xs text-muted-foreground">
                  Categorised outflows on these accounts become the business expenses.
                </p>
              </div>
              <label className="flex items-center justify-between rounded-md border px-3 py-2">
                <span className="text-sm font-medium">Sync sales from the shop app</span>
                <Switch size="sm" checked={shopSync} onCheckedChange={setShopSync} />
              </label>
            </>
          )}
          {error && <p className="text-xs text-destructive">{error}</p>}
        </div>
        <DialogFooter>
          {isEdit && (
            <Button variant="ghost" size="sm" className="mr-auto text-destructive" onClick={() => setConfirmDelete(true)} disabled={saving}>
              Delete source
            </Button>
          )}
          <Button variant="outline" size="sm" onClick={onClose}>Cancel</Button>
          <Button size="sm" onClick={() => void handleSave()} disabled={!valid || saving}>
            {saving ? "Saving..." : isEdit ? "Save" : "Create"}
          </Button>
        </DialogFooter>
        <ConfirmDialog
          open={confirmDelete}
          onOpenChange={setConfirmDelete}
          title="Delete income source?"
          busy={saving}
          error={error}
          onConfirm={() => void handleDelete()}
        >
          Sources with records cannot be deleted: remove their records first.
        </ConfirmDialog>
      </DialogContent>
    </Dialog>
  )
}
