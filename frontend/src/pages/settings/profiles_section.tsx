import { useState } from "react"
import { api } from "@/api/client"
import type { Profile } from "@/types"
import type { RemoteData } from "@/lib/remote_data"
import { visitRemoteData } from "@/lib/remote_data"
import { SettingsListSkeleton } from "@/components/skeletons"
import { NonIdealState } from "@/components/non_ideal_state"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { Trash2, Pencil, Plus, User } from "lucide-react"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"

export function ProfilesSection({ data, onRefresh }: { data: RemoteData<Profile[]>; onRefresh: () => void }) {
  return visitRemoteData(data, {
    notLoaded: () => <ProfilesCard loading onRefresh={onRefresh}><SettingsListSkeleton rows={3} /></ProfilesCard>,
    failed: (error) => <ProfilesCard onRefresh={onRefresh}><NonIdealState title="Could not load profiles" description={error} action={{ label: "Try again", onClick: onRefresh }} /></ProfilesCard>,
    hasValue: (profiles) => <ProfilesCard onRefresh={onRefresh}><ProfilesList profiles={profiles} /></ProfilesCard>,
  })
}

function ProfilesCard({ children, loading, onRefresh }: { children: React.ReactNode; loading?: boolean; onRefresh: () => void }) {
  return (
    <Card id="profiles">
      <CardHeader>
        <div className="flex items-center justify-between">
          <CardTitle className="text-lg">Profiles</CardTitle>
          {!loading && <AddProfileButton onRefresh={onRefresh} />}
        </div>
        <p className="text-sm text-muted-foreground">
          Each profile represents a person. Accounts and transactions are scoped to profiles.
        </p>
      </CardHeader>
      <CardContent>{children}</CardContent>
    </Card>
  )
}

function ProfilesList({ profiles }: { profiles: Profile[] }) {
  if (profiles.length === 0) return (
    <p className="text-sm text-muted-foreground py-4 text-center">No profiles yet. Create one to get started.</p>
  )
  return (
    <div className="space-y-2">
      {profiles.map((p) => (
        <div key={p.id} className="flex items-center gap-3 rounded-lg border p-3">
          <div className="flex h-8 w-8 items-center justify-center rounded-full bg-secondary">
            <User className="h-4 w-4 text-muted-foreground" />
          </div>
          <div className="flex-1 min-w-0">
            <p className="text-sm font-medium">{p.name}</p>
            <p className="text-xs text-muted-foreground">{p.id}</p>
          </div>
          <Tooltip>
            <TooltipTrigger render={<Button variant="ghost" size="icon" className="h-8 w-8" disabled />}>
              <Pencil className="h-3.5 w-3.5" />
            </TooltipTrigger>
            <TooltipContent>Edit coming soon</TooltipContent>
          </Tooltip>
          <Tooltip>
            <TooltipTrigger render={<Button variant="ghost" size="icon" className="h-8 w-8" disabled />}>
              <Trash2 className="h-3.5 w-3.5" />
            </TooltipTrigger>
            <TooltipContent>Delete coming soon</TooltipContent>
          </Tooltip>
        </div>
      ))}
    </div>
  )
}

function AddProfileButton({ onRefresh }: { onRefresh: () => void }) {
  const [showAdd, setShowAdd] = useState(false)
  const [name, setName] = useState("")
  const [id, setId] = useState("")
  const [creating, setCreating] = useState(false)

  function slugify(text: string) {
    return text.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "")
  }

  async function handleCreate() {
    if (!name.trim() || !id.trim()) return
    setCreating(true)
    try {
      await api.createProfile({ id: id.trim(), name: name.trim() })
      setShowAdd(false)
      setName("")
      setId("")
      onRefresh()
    } finally {
      setCreating(false)
    }
  }

  return (
    <>
      <Button size="sm" className="gap-1.5" onClick={() => setShowAdd(true)}>
        <Plus className="h-3.5 w-3.5" /> Add Profile
      </Button>
      <Dialog open={showAdd} onOpenChange={setShowAdd}>
        <DialogContent className="sm:max-w-sm">
          <DialogHeader><DialogTitle>Add Profile</DialogTitle></DialogHeader>
          <div className="space-y-3 pt-2">
            <div>
              <label className="text-sm font-medium">Name</label>
              <Input placeholder="e.g. Alex" value={name} onChange={(e) => { setName(e.target.value); setId(slugify(e.target.value)) }} autoFocus />
            </div>
            <div>
              <label className="text-sm font-medium">ID</label>
              <Input placeholder="e.g. alex" value={id} onChange={(e) => setId(e.target.value)} />
              <p className="text-xs text-muted-foreground mt-1">Unique identifier, auto-generated from name</p>
            </div>
            <div className="flex justify-end gap-2">
              <Button variant="outline" size="sm" onClick={() => setShowAdd(false)}>Cancel</Button>
              <Button size="sm" onClick={handleCreate} disabled={!name.trim() || !id.trim() || creating}>
                {creating ? "Creating..." : "Create"}
              </Button>
            </div>
          </div>
        </DialogContent>
      </Dialog>
    </>
  )
}
