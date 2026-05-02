import { useState } from "react"
import { api } from "@/api/client"
import type { Account, Profile, AccountType } from "@/types"
import type { RemoteData } from "@/lib/remote_data"
import { visitRemoteData } from "@/lib/remote_data"
import { useIngestionPreferences } from "@/hooks/use_ingestion_preferences"
import { DraggableList, DragHandle } from "@/components/draggable_list"
import { SettingsListSkeleton } from "@/components/skeletons"
import { NonIdealState } from "@/components/non_ideal_state"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { Badge } from "@/components/ui/badge"
import { Trash2, Pencil, Plus, Building2, Eye, EyeOff } from "lucide-react"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"

const ACCOUNT_TYPES: AccountType[] = [
  "checking", "savings", "investment", "credit", "cash", "pension", "property", "mortgage",
]

interface Props {
  data: RemoteData<Account[]>
  profilesData: RemoteData<Profile[]>
  onRefresh: () => void
}

export function AccountsSection({ data, profilesData, onRefresh }: Props) {
  const profiles = profilesData.status === "succeeded" || profilesData.status === "reloading"
    ? profilesData.value : []

  return (
    <Card id="accounts">
      <CardHeader>
        <div className="flex items-center justify-between">
          <CardTitle className="text-lg">Accounts</CardTitle>
          {(data.status === "succeeded" || data.status === "reloading") && (
            <AddAccountButton profiles={profiles} onRefresh={onRefresh} />
          )}
        </div>
        <p className="text-sm text-muted-foreground">
          Bank accounts, investment accounts, credit cards, and other financial accounts.
          Drag to set the import wizard order. Toggle visibility to include or exclude from the wizard.
        </p>
      </CardHeader>
      <CardContent>
        {visitRemoteData(data, {
          notLoaded: () => <SettingsListSkeleton rows={4} />,
          failed: (error) => <NonIdealState title="Could not load accounts" description={error} action={{ label: "Try again", onClick: onRefresh }} />,
          hasValue: (accounts) => <AccountsList accounts={accounts} />,
        })}
      </CardContent>
    </Card>
  )
}

function AccountsList({ accounts }: { accounts: Account[] }) {
  const [dragId, setDragId] = useState<string | null>(null)
  const {
    getOrderedAccounts,
    getHiddenAccounts,
    showAccount,
    hideAccount,
    reorderAccounts,
  } = useIngestionPreferences()

  if (accounts.length === 0) return (
    <p className="text-sm text-muted-foreground py-4 text-center">No accounts yet.</p>
  )

  const visibleAccounts = getOrderedAccounts(accounts)
  const hiddenAccounts = getHiddenAccounts(accounts)

  // DraggableList requires items with an `id` field — use account.id (not index)
  const draggableItems = visibleAccounts.map(a => ({ ...a, id: a.id }))

  return (
    <div className="space-y-4">
      <DraggableList
        items={draggableItems}
        dragId={dragId}
        onDragChange={setDragId}
        onReorder={reorderAccounts}
        listClassName="space-y-2"
        renderItem={(a) => (
          <div className="flex items-center gap-3 rounded-lg border p-3 bg-background group">
            <DragHandle />
            <div className="flex h-8 w-8 items-center justify-center rounded-full bg-secondary shrink-0">
              <Building2 className="h-4 w-4 text-muted-foreground" />
            </div>
            <div className="flex-1 min-w-0">
              <div className="flex items-center gap-2">
                <p className="text-sm font-medium truncate">{a.name}</p>
                <Badge variant="secondary" className="text-[10px] capitalize">{a.type}</Badge>
              </div>
              <p className="text-xs text-muted-foreground">{a.institution} &middot; {a.currency}</p>
            </div>
            {a.balance && (
              <p className="text-sm font-medium tabular-nums shrink-0">
                {a.currency === "GBP" ? "£" : a.currency} {parseFloat(a.balance).toLocaleString("en-GB", { minimumFractionDigits: 2 })}
              </p>
            )}
            <Tooltip>
              <TooltipTrigger render={<Button variant="ghost" size="icon" className="h-8 w-8 shrink-0" onClick={() => hideAccount(a.id, accounts)} />}>
                <Eye className="h-3.5 w-3.5" />
              </TooltipTrigger>
              <TooltipContent>Hide from import wizard</TooltipContent>
            </Tooltip>
            <Tooltip>
              <TooltipTrigger render={<Button variant="ghost" size="icon" className="h-8 w-8 shrink-0" disabled />}>
                <Pencil className="h-3.5 w-3.5" />
              </TooltipTrigger>
              <TooltipContent>Edit coming soon</TooltipContent>
            </Tooltip>
            <Tooltip>
              <TooltipTrigger render={<Button variant="ghost" size="icon" className="h-8 w-8 shrink-0" disabled />}>
                <Trash2 className="h-3.5 w-3.5" />
              </TooltipTrigger>
              <TooltipContent>Delete coming soon</TooltipContent>
            </Tooltip>
          </div>
        )}
      />

      {hiddenAccounts.length > 0 && (
        <div>
          <p className="text-xs font-medium text-muted-foreground uppercase tracking-wider mb-2">
            Hidden from import wizard ({hiddenAccounts.length})
          </p>
          <div className="space-y-2">
            {hiddenAccounts.map(a => (
              <div key={a.id} className="flex items-center gap-3 rounded-lg border border-dashed p-3 opacity-60 hover:opacity-100 transition-opacity">
                <div className="flex h-8 w-8 items-center justify-center rounded-full bg-secondary shrink-0">
                  <Building2 className="h-4 w-4 text-muted-foreground" />
                </div>
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2">
                    <p className="text-sm font-medium truncate">{a.name}</p>
                    <Badge variant="secondary" className="text-[10px] capitalize">{a.type}</Badge>
                  </div>
                  <p className="text-xs text-muted-foreground">{a.institution} &middot; {a.currency}</p>
                </div>
                <Tooltip>
                  <TooltipTrigger render={<Button variant="ghost" size="icon" className="h-8 w-8 shrink-0" onClick={() => showAccount(a.id, accounts)} />}>
                    <EyeOff className="h-3.5 w-3.5" />
                  </TooltipTrigger>
                  <TooltipContent>Show in import wizard</TooltipContent>
                </Tooltip>
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  )
}

function AddAccountButton({ profiles, onRefresh }: { profiles: Profile[]; onRefresh: () => void }) {
  const [showAdd, setShowAdd] = useState(false)
  const [creating, setCreating] = useState(false)
  const [form, setForm] = useState({
    name: "", id: "", institution: "",
    type: "checking" as AccountType,
    currency: "GBP", profileIds: [] as string[], notes: "",
  })

  function slugify(text: string) {
    return text.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "")
  }

  function resetForm() {
    setForm({ name: "", id: "", institution: "", type: "checking", currency: "GBP", profileIds: [], notes: "" })
  }

  async function handleCreate() {
    if (!form.name.trim() || !form.id.trim() || !form.institution.trim()) return
    setCreating(true)
    try {
      await api.createAccount({
        id: form.id.trim(), name: form.name.trim(), institution: form.institution.trim(),
        type: form.type, currency: form.currency || "GBP",
        profile_ids: form.profileIds.length > 0 ? form.profileIds : undefined,
        notes: form.notes.trim() || undefined,
      })
      setShowAdd(false)
      resetForm()
      onRefresh()
    } finally {
      setCreating(false)
    }
  }

  function toggleProfile(profileId: string) {
    setForm(prev => ({
      ...prev,
      profileIds: prev.profileIds.includes(profileId)
        ? prev.profileIds.filter(id => id !== profileId)
        : [...prev.profileIds, profileId],
    }))
  }

  return (
    <>
      <Button size="sm" className="gap-1.5" onClick={() => { resetForm(); setShowAdd(true) }}>
        <Plus className="h-3.5 w-3.5" /> Add Account
      </Button>
      <Dialog open={showAdd} onOpenChange={setShowAdd}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader><DialogTitle>Add Account</DialogTitle></DialogHeader>
          <div className="space-y-3 pt-2">
            <div>
              <label className="text-sm font-medium">Name</label>
              <Input placeholder="e.g. Monzo Current" value={form.name}
                onChange={(e) => setForm(f => ({ ...f, name: e.target.value, id: slugify(e.target.value) }))} autoFocus />
            </div>
            <div>
              <label className="text-sm font-medium">ID</label>
              <Input placeholder="e.g. monzo-current" value={form.id}
                onChange={(e) => setForm(f => ({ ...f, id: e.target.value }))} />
            </div>
            <div>
              <label className="text-sm font-medium">Institution</label>
              <Input placeholder="e.g. Monzo" value={form.institution}
                onChange={(e) => setForm(f => ({ ...f, institution: e.target.value }))} />
            </div>
            <div className="grid grid-cols-2 gap-3">
              <div>
                <label className="text-sm font-medium">Type</label>
                <Select value={form.type} onValueChange={(v) => setForm(f => ({ ...f, type: v as AccountType }))}>
                  <SelectTrigger><SelectValue /></SelectTrigger>
                  <SelectContent>
                    {ACCOUNT_TYPES.map(t => <SelectItem key={t} value={t} className="capitalize">{t}</SelectItem>)}
                  </SelectContent>
                </Select>
              </div>
              <div>
                <label className="text-sm font-medium">Currency</label>
                <Input placeholder="GBP" value={form.currency}
                  onChange={(e) => setForm(f => ({ ...f, currency: e.target.value.toUpperCase() }))} />
              </div>
            </div>
            {profiles.length > 0 && (
              <div>
                <label className="text-sm font-medium">Profiles</label>
                <div className="flex flex-wrap gap-1.5 mt-1">
                  {profiles.map(p => (
                    <Badge key={p.id} variant={form.profileIds.includes(p.id) ? "default" : "outline"}
                      className="cursor-pointer" onClick={() => toggleProfile(p.id)}>
                      {p.name}
                    </Badge>
                  ))}
                </div>
              </div>
            )}
            <div>
              <label className="text-sm font-medium">Notes (optional)</label>
              <Input placeholder="Any additional notes" value={form.notes}
                onChange={(e) => setForm(f => ({ ...f, notes: e.target.value }))} />
            </div>
            <div className="flex justify-end gap-2">
              <Button variant="outline" size="sm" onClick={() => setShowAdd(false)}>Cancel</Button>
              <Button size="sm" onClick={handleCreate}
                disabled={!form.name.trim() || !form.id.trim() || !form.institution.trim() || creating}>
                {creating ? "Creating..." : "Create"}
              </Button>
            </div>
          </div>
        </DialogContent>
      </Dialog>
    </>
  )
}
