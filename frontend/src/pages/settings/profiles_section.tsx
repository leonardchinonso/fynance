import { useState } from "react"
import { api } from "@/api/client"
import type { Profile } from "@/types"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { Trash2, Pencil, Plus, User } from "lucide-react"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"

interface Props {
  profiles: Profile[]
  onRefresh: () => void
}

export function ProfilesSection({ profiles, onRefresh }: Props) {
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
    } catch (err) {
      console.error("Failed to create profile:", err)
    } finally {
      setCreating(false)
    }
  }

  return (
    <Card id="profiles">
      <CardHeader>
        <div className="flex items-center justify-between">
          <CardTitle className="text-lg">Profiles</CardTitle>
          <Button size="sm" className="gap-1.5" onClick={() => setShowAdd(true)}>
            <Plus className="h-3.5 w-3.5" /> Add Profile
          </Button>
        </div>
        <p className="text-sm text-muted-foreground">
          Each profile represents a person. Accounts and transactions are scoped to profiles.
        </p>
      </CardHeader>
      <CardContent>
        {profiles.length === 0 ? (
          <p className="text-sm text-muted-foreground py-4 text-center">No profiles yet. Create one to get started.</p>
        ) : (
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
                  <TooltipTrigger asChild>
                    <Button variant="ghost" size="icon" className="h-8 w-8" disabled>
                      <Pencil className="h-3.5 w-3.5" />
                    </Button>
                  </TooltipTrigger>
                  <TooltipContent>Edit coming soon</TooltipContent>
                </Tooltip>
                <Tooltip>
                  <TooltipTrigger asChild>
                    <Button variant="ghost" size="icon" className="h-8 w-8" disabled>
                      <Trash2 className="h-3.5 w-3.5" />
                    </Button>
                  </TooltipTrigger>
                  <TooltipContent>Delete coming soon</TooltipContent>
                </Tooltip>
              </div>
            ))}
          </div>
        )}
      </CardContent>

      <Dialog open={showAdd} onOpenChange={setShowAdd}>
        <DialogContent className="sm:max-w-sm">
          <DialogHeader>
            <DialogTitle>Add Profile</DialogTitle>
          </DialogHeader>
          <div className="space-y-3 pt-2">
            <div>
              <label className="text-sm font-medium">Name</label>
              <Input
                placeholder="e.g. Alex"
                value={name}
                onChange={(e) => {
                  setName(e.target.value)
                  setId(slugify(e.target.value))
                }}
                autoFocus
              />
            </div>
            <div>
              <label className="text-sm font-medium">ID</label>
              <Input
                placeholder="e.g. alex"
                value={id}
                onChange={(e) => setId(e.target.value)}
              />
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
    </Card>
  )
}
