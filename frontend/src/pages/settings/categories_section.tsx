import { useState, useRef, useEffect } from "react"
import { api } from "@/api/client"
import type { CategoryNode } from "@/bindings/CategoryNode"
import type { CategoryType } from "@/bindings/CategoryType"
import { CATEGORY_TYPE_LABELS } from "@/bindings/category_type_groups"
import { CATEGORY_TYPE_GROUPS, colorForType } from "@/lib/category_types"
import { visitRemoteData } from "@/lib/remote_data"
import {
  DropdownMenu, DropdownMenuTrigger, DropdownMenuContent, DropdownMenuItem,
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
import { Trash2, Pencil, Plus, Tag, ChevronRight, ChevronDown, CircleHelp } from "lucide-react"
import { Tooltip, TooltipTrigger, TooltipContent, TooltipProvider } from "@/components/ui/tooltip"
import { cn } from "@/lib/utils"
import { DIALOG_FIELD_CLASS } from "@/lib/field_styles"

const DEFAULT_GROUP_COLOR = "#78716c"

const CAT_TYPE_DESC: Record<string, string> = {
  spending: "money going out",
  income: "money coming in (e.g. salary)",
  interest: "interest & investment income",
  donation: "charitable giving & gifts",
  internal_transfer: "between your own accounts; excluded from summaries",
}

type DialogState = { mode: "group" | "category"; editId: string | null }
type FormState = { name: string; parent_id: string; description: string; category_type: CategoryType; color: string }

const EMPTY_FORM: FormState = { name: "", parent_id: "", description: "", category_type: "spending", color: COLOR_PALETTE[0] }

export function CategoriesSection() {
  const [categoriesData, refresh] = useCategories()
  const [dialog, setDialog] = useState<DialogState | null>(null)
  const [form, setForm] = useState<FormState>(EMPTY_FORM)
  const [saving, setSaving] = useState(false)

  const tree = categoriesData.status === "succeeded" || categoriesData.status === "reloading"
    ? categoriesData.value : []

  const parentNames = tree.map(n => n.name)
  const { categoryColors, syncParents, setColor } = useCategoryColorsContext()

  useEffect(() => {
    if (parentNames.length > 0) syncParents(parentNames)
  }, [parentNames.join(",")])

  const loaded = categoriesData.status === "succeeded" || categoriesData.status === "reloading"

  async function handleSave() {
    if (!form.name.trim() || !dialog) return
    setSaving(true)
    try {
      if (dialog.mode === "group") {
        if (dialog.editId) {
          await api.updateCategory(dialog.editId, { name: form.name.trim() })
        } else {
          await api.createCategory({ name: form.name.trim(), category_type: "spending" })
        }
        setColor(form.name.trim(), form.color)
      } else if (dialog.editId) {
        await api.updateCategory(dialog.editId, {
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
      setDialog(null)
      refresh()
    } finally {
      setSaving(false)
    }
  }

  async function handleDelete(id: string) {
    await api.deleteCategory(id)
    refresh()
  }

  function openGroupAdd() {
    setForm({ ...EMPTY_FORM, color: COLOR_PALETTE[0] })
    setDialog({ mode: "group", editId: null })
  }

  function openCategoryAdd() {
    setForm({ ...EMPTY_FORM, parent_id: tree[0]?.id ?? "" })
    setDialog({ mode: "category", editId: null })
  }

  function openEdit(node: CategoryNode, parentId: string | null) {
    if (parentId === null) {
      setForm({ ...EMPTY_FORM, name: node.name, color: categoryColors[node.name] ?? DEFAULT_GROUP_COLOR })
      setDialog({ mode: "group", editId: node.id })
    } else {
      setForm({ ...EMPTY_FORM, name: node.name, parent_id: parentId, description: node.description ?? "", category_type: node.category_type })
      setDialog({ mode: "category", editId: node.id })
    }
  }

  const isGroup = dialog?.mode === "group"
  const title = isGroup
    ? (dialog?.editId ? "Edit group" : "New group")
    : (dialog?.editId ? "Edit category" : "New category")

  return (
    <Card id="categories">
      <CardHeader>
        <div className="flex items-center justify-between">
          <CardTitle className="text-lg">Categories</CardTitle>
          {loaded && (
            <div className="flex items-center gap-2">
              <Button size="sm" variant="outline" className="gap-1.5" onClick={openGroupAdd}>
                <Plus className="h-3.5 w-3.5" /> New group
              </Button>
              <Button size="sm" className="gap-1.5" onClick={openCategoryAdd}>
                <Plus className="h-3.5 w-3.5" /> New category
              </Button>
            </div>
          )}
        </div>
        <p className="text-sm text-muted-foreground">
          Categories are organized into groups. Budgets are set in the Budget view.
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

      <Dialog open={dialog !== null} onOpenChange={(open) => { if (!open) setDialog(null) }}>
        <DialogContent className="sm:max-w-sm p-6">
          <DialogHeader><DialogTitle>{title}</DialogTitle></DialogHeader>
          <div className="space-y-4 pt-1">
            <div>
              <label className="text-sm font-medium">Name</label>
              <Input
                className="mt-1.5"
                placeholder={isGroup ? "e.g. Housing" : "e.g. Groceries"}
                value={form.name}
                onChange={(e) => setForm(f => ({ ...f, name: e.target.value }))}
                autoFocus
              />
            </div>

            {isGroup ? (
              <div>
                <label className="text-sm font-medium">Color</label>
                <div className="mt-1.5">
                  <CategoryColorPicker
                    name={form.name || "Group"}
                    color={form.color}
                    onChange={(_, c) => setForm(f => ({ ...f, color: c }))}
                  />
                </div>
              </div>
            ) : (
              <>
                <div>
                  <label className="text-sm font-medium">Group</label>
                  <div className="relative mt-1.5">
                    <select
                      className={cn(DIALOG_FIELD_CLASS, "appearance-none pr-8")}
                      value={form.parent_id}
                      onChange={(e) => setForm(f => ({ ...f, parent_id: e.target.value }))}
                    >
                      <option value="">None (top-level)</option>
                      {tree.map(node => (
                        <option key={node.id} value={node.id}>{node.name}</option>
                      ))}
                    </select>
                    <ChevronDown className="pointer-events-none absolute right-2.5 top-1/2 h-4 w-4 -translate-y-1/2 opacity-50" />
                  </div>
                </div>
                <div>
                  <div className="flex items-center gap-1.5">
                    <label className="text-sm font-medium">Type</label>
                    <TooltipProvider delay={150}>
                      <Tooltip>
                        <TooltipTrigger
                          className="text-muted-foreground hover:text-foreground"
                          aria-label="About category types"
                        >
                          <CircleHelp className="h-3.5 w-3.5" />
                        </TooltipTrigger>
                        <TooltipContent side="top" className="max-w-sm">
                          <div className="space-y-2 text-left">
                            <p className="text-sm font-semibold">Category types</p>
                            <p className="text-muted-foreground">How money moves — drives tax and summary calculations.</p>
                            <ul className="space-y-1.5">
                              {CATEGORY_TYPE_GROUPS.map((g) => (
                                <li key={g.key} className="flex items-start gap-2">
                                  <span className="mt-1 h-2 w-2 shrink-0 rounded-full" style={{ backgroundColor: g.color }} />
                                  <span>
                                    <span className="font-medium">{g.label}</span>
                                    {" "}— {CAT_TYPE_DESC[g.key]}
                                  </span>
                                </li>
                              ))}
                            </ul>
                            <p className="text-muted-foreground">
                              Income, Interest &amp; Donation have a <span className="font-medium text-foreground">taxed / non-tax</span> split for tax-return help.
                            </p>
                          </div>
                        </TooltipContent>
                      </Tooltip>
                    </TooltipProvider>
                  </div>
                  <TypeTagPicker value={form.category_type} onChange={(t) => setForm(f => ({ ...f, category_type: t }))} />
                </div>
                <div>
                  <label className="text-sm font-medium">Description <span className="text-muted-foreground font-normal">(optional)</span></label>
                  <textarea
                    className={cn(DIALOG_FIELD_CLASS, "mt-1.5 resize-y min-h-[7.5rem] placeholder:text-muted-foreground/50")}
                    rows={5}
                    placeholder="e.g. Utility bills — internet, water, gas, electricity. Not Netflix or Spotify."
                    value={form.description}
                    onChange={(e) => setForm(f => ({ ...f, description: e.target.value }))}
                  />
                </div>
              </>
            )}

            <div className="flex justify-end gap-2">
              <Button variant="outline" size="sm" onClick={() => setDialog(null)}>Cancel</Button>
              <Button size="sm" onClick={handleSave} disabled={!form.name.trim() || saving}>
                {saving ? "Saving..." : dialog?.editId ? "Update" : "Create"}
              </Button>
            </div>
          </div>
        </DialogContent>
      </Dialog>
    </Card>
  )
}

/** Bracket appended to a selected type tag, e.g. "Income (taxed)". */
function subtypeSuffix(t: CategoryType): string {
  if (t.includes("non_taxable")) return " (non tax)"
  if (t.includes("_taxable")) return " (taxed)"
  return ""
}

/**
 * Type selector as a row of colored tags. Inactive tags are dashed + dimmed;
 * the selected one is filled. Groups with a taxable / non-taxable split open a
 * submenu, and only commit once a subtype is chosen.
 */
function TypeTagPicker({ value, onChange }: { value: CategoryType; onChange: (t: CategoryType) => void }) {
  return (
    <div className="mt-1.5 flex flex-wrap gap-2">
      {CATEGORY_TYPE_GROUPS.map((g) => {
        const selected = g.types.includes(value)
        const label = g.label + (selected ? subtypeSuffix(value) : "")
        const className = cn(
          "rounded-full border px-3 py-1 text-xs font-medium transition-all",
          selected ? "border-solid" : "border-dashed bg-transparent opacity-60 hover:opacity-100",
        )
        const style = selected
          ? { backgroundColor: g.color, borderColor: g.color, color: "#fff" }
          : { color: g.color, borderColor: g.color }

        if (g.types.length === 1) {
          return (
            <button key={g.key} type="button" className={className} style={style} onClick={() => onChange(g.types[0])}>
              {label}
            </button>
          )
        }
        return (
          <DropdownMenu key={g.key}>
            <DropdownMenuTrigger className={className} style={style}>
              {label}
            </DropdownMenuTrigger>
            <DropdownMenuContent>
              {g.types.map((t) => (
                <DropdownMenuItem key={t} onClick={() => onChange(t)}>
                  {t.includes("non_taxable") ? "Non-taxable" : "Taxable"}
                </DropdownMenuItem>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>
        )
      })}
    </div>
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
  const [open, setOpen] = useState<Record<string, boolean>>({})
  if (nodes.length === 0) return (
    <p className="text-sm text-muted-foreground py-4 text-center">No groups yet.</p>
  )
  const toggle = (id: string) => setOpen(o => ({ ...o, [id]: !o[id] }))
  return (
    <div className="space-y-4">
      {nodes.map(parent => {
        const isOpen = open[parent.id] ?? false
        return (
        <div key={parent.id}>
          <div className="flex items-center gap-3 rounded-lg border p-2.5 group bg-muted/30">
            <button
              type="button"
              onClick={() => toggle(parent.id)}
              className="shrink-0 text-muted-foreground"
              aria-label={isOpen ? `Collapse ${parent.name}` : `Expand ${parent.name}`}
              aria-expanded={isOpen}
            >
              <ChevronRight className={cn("h-4 w-4 transition-transform", isOpen && "rotate-90")} />
            </button>
            <CategoryColorPicker
              name={parent.name}
              color={categoryColors[parent.name] ?? DEFAULT_GROUP_COLOR}
              onChange={onColorChange}
            />
            <button
              type="button"
              onClick={() => toggle(parent.id)}
              className="flex flex-1 min-w-0 items-center gap-2 text-left"
            >
              <span className="truncate text-sm font-semibold">{parent.name}</span>
              {parent.children.length > 0 && (
                <span className="text-xs text-muted-foreground">({parent.children.length})</span>
              )}
            </button>
            <Button variant="ghost" size="icon" className="h-7 w-7 opacity-0 group-hover:opacity-100" onClick={() => onEdit(parent, null)}>
              <Pencil className="h-3 w-3" />
            </Button>
            <Button variant="ghost" size="icon" className="h-7 w-7 opacity-0 group-hover:opacity-100" onClick={() => onDelete(parent.id)}>
              <Trash2 className="h-3 w-3" />
            </Button>
          </div>
          {isOpen && parent.children.length > 0 && (
            <div className="ml-4 mt-1 space-y-1">
              {parent.children.map(child => (
                <div key={child.id} className="flex items-center gap-3 rounded-lg border p-2.5 group">
                  <Tag className="h-3.5 w-3.5 text-muted-foreground shrink-0" />
                  <p className="flex-1 text-sm">{child.name}</p>
                  <Badge
                    variant="secondary"
                    className="text-[10px] font-normal"
                    style={{
                      color: colorForType(child.category_type),
                      backgroundColor: `${colorForType(child.category_type)}1f`,
                    }}
                  >
                    {CATEGORY_TYPE_LABELS[child.category_type]}
                  </Badge>
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
        )
      })}
    </div>
  )
}
