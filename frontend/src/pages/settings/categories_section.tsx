import { useState, useRef, useEffect } from "react"
import { api } from "@/api/client"
import type { CategoryNode } from "@/bindings/CategoryNode"
import type { CategoryType } from "@/bindings/CategoryType"
import { CATEGORY_TYPE_LABELS } from "@/bindings/category_type_groups"
import { CATEGORY_TYPE_GROUPS } from "@/lib/category_types"
import { visitRemoteData } from "@/lib/remote_data"
import {
  DropdownMenu, DropdownMenuTrigger, DropdownMenuContent, DropdownMenuItem,
  DropdownMenuSub, DropdownMenuSubTrigger, DropdownMenuSubContent,
} from "@/components/ui/dropdown-menu"
import { useCategories } from "@/hooks/data"
import { useCategoryColorsContext } from "@/context/category_colors_context"
import { COLOR_PALETTE } from "@/lib/colors"
import { SettingsListSkeleton } from "@/components/skeletons"
import { AuthAwareError } from "@/components/auth_aware_error"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { Badge } from "@/components/ui/badge"
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover"
import { Trash2, Pencil, Plus, Tag, ChevronDown } from "lucide-react"

export function CategoriesSection() {
  const [categoriesData, refresh] = useCategories()
  const [showAdd, setShowAdd] = useState(false)
  const [editCat, setEditCat] = useState<{ id: string; name: string; parent_id: string | null; description: string | null } | null>(null)
  const [form, setForm] = useState<{ name: string; parent_id: string; description: string; category_type: CategoryType }>(
    { name: "", parent_id: "", description: "", category_type: "spending" },
  )
  const [saving, setSaving] = useState(false)

  const tree = categoriesData.status === "succeeded" || categoriesData.status === "reloading"
    ? categoriesData.value : []

  const parentNames = tree.map(n => n.name)
  const { categoryColors, syncParents, setColor } = useCategoryColorsContext()

  useEffect(() => {
    if (parentNames.length > 0) syncParents(parentNames)
  }, [parentNames.join(",")])

  async function handleSave() {
    if (!form.name.trim()) return
    setSaving(true)
    try {
      if (editCat) {
        await api.updateCategory(editCat.id, {
          name: form.name.trim(),
          parent_id: form.parent_id || undefined,
          // Empty string = explicit "clear" on the backend; an unchanged
          // value still round-trips because the API treats omitted as
          // "leave alone".
          description: form.description.trim(),
          category_type: form.category_type,
        })
      } else {
        await api.createCategory({
          name: form.name.trim(),
          parent_id: form.parent_id || undefined,
          description: form.description.trim() || undefined,
          category_type: form.category_type,
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
    setEditCat({ id: node.id, name: node.name, parent_id: parentId, description: node.description ?? null })
    setForm({ name: node.name, parent_id: parentId ?? "", description: node.description ?? "", category_type: node.category_type })
    setShowAdd(true)
  }

  return (
    <Card id="categories">
      <CardHeader>
        <div className="flex items-center justify-between">
          <CardTitle className="text-lg">Categories</CardTitle>
          {(categoriesData.status === "succeeded" || categoriesData.status === "reloading") && (
            <Button size="sm" className="gap-1.5" onClick={() => { setEditCat(null); setForm({ name: "", parent_id: "", description: "", category_type: "spending" }); setShowAdd(true) }}>
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
          failed: (error) => <AuthAwareError error={error} onRetry={refresh} />,
          hasValue: (nodes) => (
            <CategoryTree
              nodes={nodes}
              onEdit={openEdit}
              onDelete={handleDelete}
              categoryColors={categoryColors}
              onColorChange={setColor}
            />
          ),
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
            <div>
              <label className="text-sm font-medium">Category type</label>
              <DropdownMenu>
                <DropdownMenuTrigger className="w-full mt-1 flex items-center justify-between rounded-md border bg-background px-3 py-2 text-sm hover:bg-accent/50">
                  <span>{CATEGORY_TYPE_LABELS[form.category_type]}</span>
                  <ChevronDown className="h-4 w-4 opacity-50" />
                </DropdownMenuTrigger>
                <DropdownMenuContent className="min-w-[14rem]">
                  {CATEGORY_TYPE_GROUPS.map((g) =>
                    g.types.length === 1 ? (
                      <DropdownMenuItem
                        key={g.key}
                        onClick={() => setForm((f) => ({ ...f, category_type: g.types[0] }))}
                      >
                        {g.label}
                      </DropdownMenuItem>
                    ) : (
                      <DropdownMenuSub key={g.key}>
                        <DropdownMenuSubTrigger>{g.label}</DropdownMenuSubTrigger>
                        <DropdownMenuSubContent>
                          {g.types.map((t) => (
                            <DropdownMenuItem
                              key={t}
                              onClick={() => setForm((f) => ({ ...f, category_type: t }))}
                            >
                              {CATEGORY_TYPE_LABELS[t]}
                            </DropdownMenuItem>
                          ))}
                        </DropdownMenuSubContent>
                      </DropdownMenuSub>
                    ),
                  )}
                </DropdownMenuContent>
              </DropdownMenu>
            </div>
            <div>
              <label className="text-sm font-medium">Description <span className="text-muted-foreground font-normal">(optional)</span></label>
              <textarea
                className="w-full mt-1 rounded-md border bg-background px-3 py-2 text-sm resize-y min-h-[7.5rem]"
                rows={5}
                placeholder="e.g. Utility bills — internet, water, gas, electricity. Not Netflix or Spotify."
                value={form.description}
                onChange={(e) => setForm(f => ({ ...f, description: e.target.value }))}
              />
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

function CategoryColorPicker({
  name,
  color,
  onChange,
}: {
  name: string
  color: string
  onChange: (name: string, color: string) => void
}) {
  const [open, setOpen] = useState(false)
  const [hexInput, setHexInput] = useState(color)
  const nativeRef = useRef<HTMLInputElement>(null)

  function applyHex(raw: string) {
    const hex = raw.startsWith("#") ? raw : "#" + raw
    if (/^#[0-9a-fA-F]{6}$/.test(hex)) {
      onChange(name, hex)
    }
  }

  return (
    <Popover open={open} onOpenChange={(o) => { setOpen(o); if (o) setHexInput(color) }}>
      <PopoverTrigger
        nativeButton={false}
        render={
          <div
            role="button"
            tabIndex={0}
            className="h-5 w-5 rounded-full border-2 border-white/20 shadow-sm ring-1 ring-black/10 shrink-0 transition-transform hover:scale-110 cursor-pointer"
            style={{ backgroundColor: color }}
            title={`Color for ${name}`}
            onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") setOpen(true) }}
          />
        }
      />
      <PopoverContent className="w-[220px] p-3" align="start">
        <p className="text-xs font-medium text-muted-foreground mb-2">{name} color</p>

        {/* Palette swatches */}
        <div className="grid grid-cols-8 gap-1 mb-3">
          {COLOR_PALETTE.map((c) => (
            <button
              key={c}
              className="h-6 w-6 rounded-full border-2 transition-transform hover:scale-110"
              style={{
                backgroundColor: c,
                borderColor: c === color ? "white" : "transparent",
                boxShadow: c === color ? `0 0 0 1px ${c}` : undefined,
              }}
              onClick={() => { onChange(name, c); setHexInput(c); setOpen(false) }}
            />
          ))}
        </div>

        {/* Native color picker */}
        <div className="flex items-center gap-2">
          <button
            className="h-8 w-8 rounded border shrink-0 overflow-hidden p-0"
            style={{ backgroundColor: color }}
            onClick={() => nativeRef.current?.click()}
            title="Open color wheel"
          >
            <input
              ref={nativeRef}
              type="color"
              className="opacity-0 w-full h-full cursor-pointer"
              value={color}
              onChange={(e) => { onChange(name, e.target.value); setHexInput(e.target.value) }}
            />
          </button>
          <Input
            className="h-8 font-mono text-xs"
            value={hexInput}
            maxLength={7}
            onChange={(e) => setHexInput(e.target.value)}
            onBlur={(e) => applyHex(e.target.value)}
            onKeyDown={(e) => { if (e.key === "Enter") applyHex(hexInput) }}
          />
        </div>
      </PopoverContent>
    </Popover>
  )
}

function CategoryTree({ nodes, onEdit, onDelete, categoryColors, onColorChange }: {
  nodes: CategoryNode[]
  onEdit: (node: CategoryNode, parentId: string | null) => void
  onDelete: (id: string) => void
  categoryColors: Record<string, string>
  onColorChange: (name: string, color: string) => void
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
            <CategoryColorPicker
              name={parent.name}
              color={categoryColors[parent.name] ?? "#78716c"}
              onChange={onColorChange}
            />
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
