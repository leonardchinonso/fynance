import { useState } from "react"
import { api } from "@/api/client"
import type { CategoryNode } from "@/bindings/CategoryNode"
import { visitRemoteData } from "@/lib/remote_data"
import { useCategories } from "@/hooks/data"
import { SettingsListSkeleton } from "@/components/skeletons"
import { NonIdealState } from "@/components/non_ideal_state"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { Badge } from "@/components/ui/badge"
import { Trash2, Pencil, Plus, Tag } from "lucide-react"

export function CategoriesSection() {
  const [categoriesData, refresh] = useCategories()
  const [showAdd, setShowAdd] = useState(false)
  const [editCat, setEditCat] = useState<{ id: string; name: string; parent_id: string | null } | null>(null)
  const [form, setForm] = useState({ name: "", parent_id: "" })
  const [saving, setSaving] = useState(false)

  const tree = categoriesData.status === "succeeded" || categoriesData.status === "reloading"
    ? categoriesData.value : []

  async function handleSave() {
    if (!form.name.trim()) return
    setSaving(true)
    try {
      if (editCat) {
        await api.updateCategory(editCat.id, {
          name: form.name.trim(),
          parent_id: form.parent_id || undefined,
        })
      } else {
        await api.createCategory({
          name: form.name.trim(),
          parent_id: form.parent_id || undefined,
        })
      }
      setShowAdd(false)
      setEditCat(null)
      refresh()
    } finally {
      setSaving(false)
    }
  }

  async function handleDelete(id: string) {
    await api.deleteCategory(id)
    refresh()
  }

  function openEdit(node: CategoryNode, parentId: string | null) {
    setEditCat({ id: node.id, name: node.name, parent_id: parentId })
    setForm({ name: node.name, parent_id: parentId ?? "" })
    setShowAdd(true)
  }

  return (
    <Card id="categories">
      <CardHeader>
        <div className="flex items-center justify-between">
          <CardTitle className="text-lg">Categories</CardTitle>
          {(categoriesData.status === "succeeded" || categoriesData.status === "reloading") && (
            <Button size="sm" className="gap-1.5" onClick={() => { setEditCat(null); setForm({ name: "", parent_id: "" }); setShowAdd(true) }}>
              <Plus className="h-3.5 w-3.5" /> Add Category
            </Button>
          )}
        </div>
        <p className="text-sm text-muted-foreground">
          Organize transactions into categories. Budgets are set in the Budget view.
        </p>
      </CardHeader>
      <CardContent>
        {visitRemoteData(categoriesData, {
          notLoaded: () => <SettingsListSkeleton rows={6} />,
          failed: (error) => <NonIdealState title="Could not load categories" description={error} action={{ label: "Try again", onClick: refresh }} />,
          hasValue: (nodes) => <CategoryTree nodes={nodes} onEdit={openEdit} onDelete={handleDelete} />,
        })}
      </CardContent>

      <Dialog open={showAdd} onOpenChange={(open) => { setShowAdd(open); if (!open) setEditCat(null) }}>
        <DialogContent className="sm:max-w-sm">
          <DialogHeader><DialogTitle>{editCat ? "Edit Category" : "Add Category"}</DialogTitle></DialogHeader>
          <div className="space-y-3 pt-2">
            <div>
              <label className="text-sm font-medium">Name</label>
              <Input placeholder="e.g. Groceries" value={form.name} onChange={(e) => setForm(f => ({ ...f, name: e.target.value }))} autoFocus />
            </div>
            <div>
              <label className="text-sm font-medium">Parent category</label>
              <select
                className="w-full mt-1 rounded-md border bg-background px-3 py-2 text-sm"
                value={form.parent_id}
                onChange={(e) => setForm(f => ({ ...f, parent_id: e.target.value }))}
              >
                <option value="">None (top-level)</option>
                {tree.map(node => (
                  <option key={node.id} value={node.id}>{node.name}</option>
                ))}
              </select>
            </div>
            <div className="flex justify-end gap-2">
              <Button variant="outline" size="sm" onClick={() => setShowAdd(false)}>Cancel</Button>
              <Button size="sm" onClick={handleSave} disabled={!form.name.trim() || saving}>
                {saving ? "Saving..." : editCat ? "Update" : "Create"}
              </Button>
            </div>
          </div>
        </DialogContent>
      </Dialog>
    </Card>
  )
}

function CategoryTree({ nodes, onEdit, onDelete }: {
  nodes: CategoryNode[]
  onEdit: (node: CategoryNode, parentId: string | null) => void
  onDelete: (id: string) => void
}) {
  if (nodes.length === 0) return (
    <p className="text-sm text-muted-foreground py-4 text-center">No categories yet.</p>
  )
  return (
    <div className="space-y-4">
      {nodes.map(parent => (
        <div key={parent.id}>
          <div className="flex items-center gap-3 rounded-lg border p-2.5 group bg-muted/30">
            <Tag className="h-3.5 w-3.5 text-muted-foreground shrink-0" />
            <p className="flex-1 text-sm font-semibold">{parent.name}</p>
            <Badge variant="outline" className="text-[10px]">parent</Badge>
            <Button variant="ghost" size="icon" className="h-7 w-7 opacity-0 group-hover:opacity-100" onClick={() => onEdit(parent, null)}>
              <Pencil className="h-3 w-3" />
            </Button>
            <Button variant="ghost" size="icon" className="h-7 w-7 opacity-0 group-hover:opacity-100" onClick={() => onDelete(parent.id)}>
              <Trash2 className="h-3 w-3" />
            </Button>
          </div>
          {parent.children.length > 0 && (
            <div className="ml-4 mt-1 space-y-1">
              {parent.children.map(child => (
                <div key={child.id} className="flex items-center gap-3 rounded-lg border p-2.5 group">
                  <Tag className="h-3.5 w-3.5 text-muted-foreground shrink-0" />
                  <p className="flex-1 text-sm">{child.name}</p>
                  <Button variant="ghost" size="icon" className="h-7 w-7 opacity-0 group-hover:opacity-100" onClick={() => onEdit(child, parent.id)}>
                    <Pencil className="h-3 w-3" />
                  </Button>
                  <Button variant="ghost" size="icon" className="h-7 w-7 opacity-0 group-hover:opacity-100" onClick={() => onDelete(child.id)}>
                    <Trash2 className="h-3 w-3" />
                  </Button>
                </div>
              ))}
            </div>
          )}
        </div>
      ))}
    </div>
  )
}
