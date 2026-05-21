import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Button } from "@/components/ui/button"

interface Counts {
  transactions: number
  holdingsCreate: number
  holdingsUpdate: number
  investments: number
}

interface Props {
  open: boolean
  onOpenChange: (open: boolean) => void
  counts: Counts
  submitting: boolean
  onConfirm: () => void
}

export function ConfirmDialog({ open, onOpenChange, counts, submitting, onConfirm }: Props) {
  const items: { label: string; count: number }[] = []
  if (counts.transactions > 0) items.push({ label: "transaction", count: counts.transactions })
  if (counts.holdingsCreate > 0) items.push({ label: "holding", count: counts.holdingsCreate })
  if (counts.holdingsUpdate > 0) items.push({ label: "holding update", count: counts.holdingsUpdate })
  if (counts.investments > 0) items.push({ label: "investment event", count: counts.investments })
  const total = items.reduce((s, x) => s + x.count, 0)

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Confirm import</DialogTitle>
          <DialogDescription>
            {total === 0
              ? "Nothing to import — every row is marked to skip."
              : "You are about to commit the following:"}
          </DialogDescription>
        </DialogHeader>

        {total > 0 && (
          <ul className="space-y-1.5 text-sm">
            {items.map((item) => (
              <li key={item.label} className="flex items-center justify-between">
                <span className="text-muted-foreground">
                  {item.label}{item.count > 1 ? "s" : ""}
                </span>
                <span className="font-medium tabular-nums">{item.count}</span>
              </li>
            ))}
          </ul>
        )}

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)} disabled={submitting}>
            Cancel
          </Button>
          <Button onClick={onConfirm} disabled={submitting || total === 0}>
            {submitting ? "Committing…" : "Confirm"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
