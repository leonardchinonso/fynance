import type { ReactNode } from "react"
import { Info } from "lucide-react"
import { Card, CardContent } from "@/components/ui/card"
import { Button } from "@/components/ui/button"

interface EmptyStateProps {
  /** Short headline. Defaults to "No matches for your filters". */
  title?: string
  /** Supporting copy. Defaults to a filter-aware message. */
  message?: string
  /** Icon element. Defaults to a muted Info icon. */
  icon?: ReactNode
  /** Optional primary action (usually "Reset filters"). */
  action?: {
    label: string
    onClick: () => void
  }
  /** Tighten the vertical padding. Use `true` inside chart cards. */
  compact?: boolean
}

/**
 * Filter-agnostic empty state used anywhere a data view has zero rows.
 * The message intentionally frames the absence as "nothing matches your
 * current view" instead of distinguishing between "DB is empty" and
 * "filters excluded everything" - both are technically true and the
 * user response is the same (adjust filters or ingest more data).
 */
export function EmptyState({
  title = "No matches for your filters",
  message = "Try widening your date range or clearing filters to see more.",
  icon,
  action,
  compact = false,
}: EmptyStateProps) {
  return (
    <Card className={compact ? "py-6" : "py-12"}>
      <CardContent className="flex flex-col items-center gap-3 text-center">
        <div className="text-muted-foreground">
          {icon ?? <Info className="h-8 w-8" />}
        </div>
        <p className="text-sm font-medium">{title}</p>
        <p className="text-xs text-muted-foreground max-w-md">{message}</p>
        {action && (
          <Button
            variant="outline"
            size="sm"
            className="mt-1"
            onClick={action.onClick}
          >
            {action.label}
          </Button>
        )}
      </CardContent>
    </Card>
  )
}
