import { useEffect, useState } from "react"
import { api } from "@/api/client"
import type { Profile } from "@/types"
import type { RemoteData } from "@/lib/remote_data"
import { visitRemoteData } from "@/lib/remote_data"
import { SettingsListSkeleton } from "@/components/skeletons"
import { AuthAwareError } from "@/components/auth_aware_error"
import { ColorSwatchPicker } from "@/components/color_swatch_picker"
import { useProfileColorsContext } from "@/context/profile_colors_context"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter } from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { Trash2, Pencil, Plus, User } from "lucide-react"

export function ProfilesSection({ data, onRefresh }: { data: RemoteData<Profile[]>; onRefresh: () => void }) {
  return visitRemoteData(data, {
    notLoaded: () => <ProfilesCard loading onRefresh={onRefresh}><SettingsListSkeleton rows={3} /></ProfilesCard>,
    failed: (error) => <ProfilesCard onRefresh={onRefresh}><AuthAwareError error={error} onRetry={onRefresh} /></ProfilesCard>,
    hasValue: (profiles) => <ProfilesCard onRefresh={onRefresh}><ProfilesList profiles={profiles} onRefresh={onRefresh} /></ProfilesCard>,
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

function ProfilesList({ profiles, onRefresh }: { profiles: Profile[]; onRefresh: () => void }) {
  const { profileColors, syncProfiles, setColor, removeColor } = useProfileColorsContext()
  const [editing, setEditing] = useState<Profile | null>(null)
  const [deleting, setDeleting] = useState<Profile | null>(null)

  useEffect(() => {
    syncProfiles(profiles.map((p) => p.id))
  }, [profiles.map((p) => p.id).join(","), syncProfiles])

  if (profiles.length === 0) return (
    <p className="text-sm text-muted-foreground py-4 text-center">No profiles yet. Create one to get started.</p>
  )

  async function handleDeleteConfirm() {
    if (!deleting) return
    try {
      await api.deleteProfile(deleting.id)
      removeColor(deleting.id)
      setDeleting(null)
      onRefresh()
    } catch (err) {
      alert(err instanceof Error ? err.message : String(err))
    }
  }

  return (
    <>
      <div className="space-y-2">
        {profiles.map((p) => (
          <div key={p.id} className="flex items-center gap-3 rounded-lg border p-3 group">
            <div
              className="flex h-8 w-8 items-center justify-center rounded-full shrink-0"
              style={{ backgroundColor: `${profileColors[p.id] ?? "#78716c"}33` }}
            >
              <User className="h-4 w-4" style={{ color: profileColors[p.id] ?? "#78716c" }} />
            </div>
            <ColorSwatchPicker
              label={`${p.name} color`}
              color={profileColors[p.id] ?? "#78716c"}
              onChange={(c) => setColor(p.id, c)}
            />
            <div className="flex-1 min-w-0">
              <p className="text-sm font-medium">{p.name}</p>
              <p className="text-xs text-muted-foreground">{p.id}</p>
            </div>
            <Button
              variant="ghost"
              size="icon"
              className="h-8 w-8 opacity-0 group-hover:opacity-100"
              onClick={() => setEditing(p)}
              title="Rename profile"
            >
              <Pencil className="h-3.5 w-3.5" />
            </Button>
            <Button
              variant="ghost"
              size="icon"
              className="h-8 w-8 opacity-0 group-hover:opacity-100"
              onClick={() => setDeleting(p)}
              disabled={p.id === "default"}
              title={p.id === "default" ? "The default profile can't be deleted" : "Delete profile"}
            >
              <Trash2 className="h-3.5 w-3.5" />
            </Button>
          </div>
        ))}
      </div>

      {editing && (
        <EditProfileDialog
          profile={editing}
          onClose={() => setEditing(null)}
          onSaved={() => { setEditing(null); onRefresh() }}
        />
      )}

      <Dialog open={!!deleting} onOpenChange={(open) => { if (!open) setDeleting(null) }}>
        <DialogContent className="sm:max-w-sm">
          <DialogHeader><DialogTitle>Delete profile?</DialogTitle></DialogHeader>
          <p className="text-sm text-muted-foreground">
            This removes <strong>{deleting?.name}</strong>. Accounts assigned to this profile must be reassigned first; otherwise the delete will fail.
          </p>
          <DialogFooter>
            <Button variant="outline" size="sm" onClick={() => setDeleting(null)}>Cancel</Button>
            <Button variant="destructive" size="sm" onClick={handleDeleteConfirm}>Delete</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  )
}

function EditProfileDialog({ profile, onClose, onSaved }: { profile: Profile; onClose: () => void; onSaved: () => void }) {
  const [name, setName] = useState(profile.name)
  const [saving, setSaving] = useState(false)

  async function handleSave() {
    if (!name.trim() || name.trim() === profile.name) {
      onClose()
      return
    }
    setSaving(true)
    try {
      await api.updateProfile(profile.id, { name: name.trim() })
      onSaved()
    } catch (err) {
      alert(err instanceof Error ? err.message : String(err))
    } finally {
      setSaving(false)
    }
  }

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="sm:max-w-sm">
        <DialogHeader><DialogTitle>Rename profile</DialogTitle></DialogHeader>
        <div className="space-y-3 pt-2">
          <div>
            <label className="text-sm font-medium">Name</label>
            <Input
              value={name}
              onChange={(e) => setName(e.target.value)}
              autoFocus
              onKeyDown={(e) => { if (e.key === "Enter") handleSave() }}
            />
          </div>
          <p className="text-xs text-muted-foreground">
            ID stays as <span className="font-mono">{profile.id}</span> (immutable).
          </p>
        </div>
        <DialogFooter>
          <Button variant="outline" size="sm" onClick={onClose}>Cancel</Button>
          <Button size="sm" onClick={handleSave} disabled={!name.trim() || saving}>
            {saving ? "Saving..." : "Save"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
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
