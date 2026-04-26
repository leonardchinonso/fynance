import { useState, useEffect } from "react"
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { api } from "@/api/client"
import { formatCurrency, categoryLeaf } from "@/lib/utils"
import { Pencil } from "lucide-react"

interface BudgetEditPopoverProps {
  category: string
  category_id: string | null
  currentBudget: string | null
  /** If provided, sets a monthly override for this period instead of the standing budget. */
  month?: string
  onSaved?: (newBudget: string) => void
}

export function BudgetEditPopover({
  category,
  category_id,
  currentBudget,
  month,
  onSaved,
}: BudgetEditPopoverProps) {
  const [open, setOpen] = useState(false)
  const [value, setValue] = useState(currentBudget ?? "")
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    setValue(currentBudget ?? "")
  }, [currentBudget])

  async function handleSave() {
    const amount = parseFloat(value)
    if (isNaN(amount) || amount < 0) {
      setError("Enter a valid amount")
      return
    }
    const formatted = amount.toFixed(2)
    if (formatted === (currentBudget ?? "")) {
      setOpen(false)
      return
    }
    setSaving(true)
    setError(null)
    try {
      const categoryArg = category_id ? null : category
      if (month) {
        await api.setBudgetOverride({ month, category_id, category: categoryArg, amount: formatted })
      } else {
        await api.setStandingBudget({ category_id, category: categoryArg, amount: formatted })
      }
      setOpen(false)
      onSaved?.(formatted)
    } catch {
      setError("Failed to save")
    } finally {
      setSaving(false)
    }
  }

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger className="group flex items-center justify-end gap-1 w-full text-right tabular-nums text-sm hover:text-foreground focus:outline-none bg-transparent border-0 p-0 cursor-pointer">
        <span>{formatCurrency(currentBudget ?? "")}</span>
        <Pencil className="h-3 w-3 opacity-0 group-hover:opacity-50 shrink-0" />
      </PopoverTrigger>
      <PopoverContent className="w-64 p-3" align="end">
        <p className="text-xs font-medium mb-1 truncate">{categoryLeaf(category)}</p>
        <p className="text-xs text-muted-foreground mb-3">
          {month ? `Override for ${month}` : "Standing monthly budget"}
        </p>
        <div className="flex gap-2">
          <Input
            type="number"
            min="0"
            step="0.01"
            placeholder="0.00"
            value={value}
            onChange={(e) => setValue(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && handleSave()}
            className="h-8 text-sm"
            autoFocus
          />
          <Button size="sm" onClick={handleSave} disabled={saving} className="h-8">
            {saving ? "..." : "Save"}
          </Button>
        </div>
        {error && <p className="text-xs text-destructive mt-2">{error}</p>}
      </PopoverContent>
    </Popover>
  )
}
