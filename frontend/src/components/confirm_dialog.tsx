import type { ReactNode } from "react"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "@/components/ui/dialog"

interface ConfirmDialogProps {
  open: boolean
  /** Close requests (backdrop, Escape, Cancel). Ignored while `busy`. */
  onOpenChange: (open: boolean) => void
  title: ReactNode
  /** Body copy explaining what the action does. */
  children: ReactNode
  confirmLabel?: string
  busyLabel?: string
  busy?: boolean
  /** Shown inline below the body, e.g. a failed previous attempt. */
  error?: string | null
  onConfirm: () => void
}

/**
 * Destructive-action confirmation dialog. One shape for every delete flow so
 * busy-guarding, close behavior, and error display cannot drift per page.
 */
export function ConfirmDialog({
  open,
  onOpenChange,
  title,
  children,
  confirmLabel = "Delete",
  busyLabel = "Deleting...",
  busy = false,
  error,
  onConfirm,
}: ConfirmDialogProps) {
  return (
    <Dialog open={open} onOpenChange={(o) => { if (!busy) onOpenChange(o) }}>
      <DialogContent className="sm:max-w-sm p-6">
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
        </DialogHeader>
        <div className="text-sm text-muted-foreground">{children}</div>
        {error && <p className="text-xs text-destructive whitespace-pre-wrap">{error}</p>}
        <div className="flex justify-end gap-2 pt-2">
          <Button variant="outline" size="sm" onClick={() => onOpenChange(false)} disabled={busy}>
            Cancel
          </Button>
          <Button variant="destructive" size="sm" onClick={onConfirm} disabled={busy}>
            {busy ? busyLabel : confirmLabel}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  )
}
