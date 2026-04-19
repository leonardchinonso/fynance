import { useState, useEffect } from "react"
import { api } from "@/api/client"
import type { CategoryDetail } from "@/types"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { Badge } from "@/components/ui/badge"
import { Trash2, Pencil, Plus, Tag } from "lucide-react"

export function CategoriesSection() {
  const [categories, setCategories] = useState<CategoryDetail[]>([])
  const [loading, setLoading] = useState(true)
  const [showAdd, setShowAdd] = useState(false)
  const [editCat, setEditCat] = useState<CategoryDetail | null>(null)
  const [form, setForm] = useState({ name: "", description: "", group: "" })
  const [saving, setSaving] = useState(false)

  async function load() {
    setLoading(true)
    try {
      const data = await api.getCategoryDetails()
      setCategories(data)
    } catch (err) {
      console.error("Failed to load categories:", err)
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => { load() }, [])

  // Group categories by their group field
  const grouped = categories.reduce<Record<string, CategoryDetail[]>>((acc, cat) => {
    const group = cat.group || "Ungrouped"
    if (!acc[group]) acc[group] = []
    acc[group].push(cat)
    return acc
  }, {})

  async function handleSave() {
    if (!form.name.trim() || !form.group.trim()) return
    setSaving(true)
    try {
      if (editCat) {
        await api.updateCategory(editCat.id, {
          name: form.name.trim(),
          description: form.description.trim(),
          group: form.group.trim(),
        })
      } else {
        await api.createCategory({
          name: form.name.trim(),
          description: form.description.trim(),
          group: form.group.trim(),
        })
      }
      setShowAdd(false)
      setEditCat(null)
      load()
    } catch (err) {
      console.error("Failed to save category:", err)
    } finally {
      setSaving(false)
    }
  }

  async function handleDelete(id: string) {
    try {
      await api.deleteCategory(id)
      load()
    } catch (err) {
      console.error("Failed to delete category:", err)
    }
  }

  function openEdit(cat: CategoryDetail) {
    setEditCat(cat)
    setForm({ name: cat.name, description: cat.description, group: cat.group })
    setShowAdd(true)
  }

  function openAdd() {
    setEditCat(null)
    setForm({ name: "", description: "", group: "" })
    setShowAdd(true)
  }

  const existingGroups = [...new Set(categories.map((c) => c.group))].sort()

  return (
    <Card id="categories">
      <CardHeader>
        <div className="flex items-center justify-between">
          <CardTitle className="text-lg">Categories</CardTitle>
          <Button size="sm" className="gap-1.5" onClick={openAdd}>
            <Plus className="h-3.5 w-3.5" /> Add Category
          </Button>
        </div>
        <p className="text-sm text-muted-foreground">
          Organize transactions into categories and groups. Budgets are set in the Budget view.
        </p>
      </CardHeader>
      <CardContent>
        {loading ? (
          <p className="text-sm text-muted-foreground py-4 text-center">Loading categories...</p>
        ) : Object.keys(grouped).length === 0 ? (
          <p className="text-sm text-muted-foreground py-4 text-center">No categories yet.</p>
        ) : (
          <div className="space-y-4">
            {Object.entries(grouped).sort(([a], [b]) => a.localeCompare(b)).map(([group, cats]) => (
              <div key={group}>
                <h4 className="text-xs font-medium text-muted-foreground uppercase tracking-wider mb-2">{group}</h4>
                <div className="space-y-1">
                  {cats.map((cat) => (
                    <div key={cat.id} className="flex items-center gap-3 rounded-lg border p-2.5 group">
                      <Tag className="h-3.5 w-3.5 text-muted-foreground shrink-0" />
                      <div className="flex-1 min-w-0">
                        <p className="text-sm font-medium">{cat.name}</p>
                        {cat.description && (
                          <p className="text-xs text-muted-foreground truncate">{cat.description}</p>
                        )}
                      </div>
                      <Badge variant="outline" className="text-[10px] shrink-0">{cat.group}</Badge>
                      <Button variant="ghost" size="icon" className="h-7 w-7 opacity-0 group-hover:opacity-100"
                        onClick={() => openEdit(cat)}>
                        <Pencil className="h-3 w-3" />
                      </Button>
                      <Button variant="ghost" size="icon" className="h-7 w-7 opacity-0 group-hover:opacity-100"
                        onClick={() => handleDelete(cat.id)}>
                        <Trash2 className="h-3 w-3" />
                      </Button>
                    </div>
                  ))}
                </div>
              </div>
            ))}
          </div>
        )}
      </CardContent>

      <Dialog open={showAdd} onOpenChange={(open) => { setShowAdd(open); if (!open) setEditCat(null) }}>
        <DialogContent className="sm:max-w-sm">
          <DialogHeader>
            <DialogTitle>{editCat ? "Edit Category" : "Add Category"}</DialogTitle>
          </DialogHeader>
          <div className="space-y-3 pt-2">
            <div>
              <label className="text-sm font-medium">Name</label>
              <Input placeholder="e.g. Groceries" value={form.name}
                onChange={(e) => setForm((f) => ({ ...f, name: e.target.value }))} autoFocus />
            </div>
            <div>
              <label className="text-sm font-medium">Description</label>
              <Input placeholder="e.g. Supermarkets and food shops" value={form.description}
                onChange={(e) => setForm((f) => ({ ...f, description: e.target.value }))} />
            </div>
            <div>
              <label className="text-sm font-medium">Group</label>
              <Input placeholder="e.g. Food" value={form.group}
                onChange={(e) => setForm((f) => ({ ...f, group: e.target.value }))}
                list="category-groups" />
              <datalist id="category-groups">
                {existingGroups.map((g) => <option key={g} value={g} />)}
              </datalist>
            </div>
            <div className="flex justify-end gap-2">
              <Button variant="outline" size="sm" onClick={() => setShowAdd(false)}>Cancel</Button>
              <Button size="sm" onClick={handleSave}
                disabled={!form.name.trim() || !form.group.trim() || saving}>
                {saving ? "Saving..." : editCat ? "Update" : "Create"}
              </Button>
            </div>
          </div>
        </DialogContent>
      </Dialog>
    </Card>
  )
}
