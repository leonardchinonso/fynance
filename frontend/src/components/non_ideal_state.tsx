import type { ReactNode } from "react"
import { AlertCircle } from "lucide-react"
import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"

interface NonIdealStateProps {
  /** Icon to display. Defaults to `AlertCircle`. */
  icon?: ReactNode
  /** Primary message — shown prominently. */
  title: string
  /** Secondary detail — shown smaller below the title. */
  description?: string
  /** Optional action button. */
  action?: { label: string; onClick: () => void }
  /**
   * Layout variant.
   *
   * - `vertical` (default) — icon stacked above title and description,
   *   horizontally centered. Use for full-section error/empty states.
   * - `horizontal` — icon, title, and description on a single line.
   *   Use inside charts or compact areas. Typically use title OR description,
   *   not both.
   */
  layout?: "vertical" | "horizontal"
  className?: string
}

/**
 * Displays a non-ideal state such as an error or empty result.
 *
 * Vertical (default):
 * ```
 *      [icon]
 *   Something went wrong
 *   Could not load data
 *      [Try again]
 * ```
 *
 * Horizontal:
 * ```
 * [icon]  Something went wrong · Could not load data
 * ```
 */
export function NonIdealState({
  icon,
  title,
  description,
  action,
  layout = "vertical",
  className,
}: NonIdealStateProps) {
  const resolvedIcon = icon ?? <AlertCircle className={layout === "vertical" ? "h-10 w-10" : "h-4 w-4"} />

  if (layout === "horizontal") {
    return (
      <div className={cn("flex items-center gap-2 text-muted-foreground", className)}>
        <span className="shrink-0">{resolvedIcon}</span>
        <span className="text-sm font-medium">{title}</span>
        {description && (
          <>
            <span className="text-muted-foreground/50">·</span>
            <span className="text-sm text-muted-foreground">{description}</span>
          </>
        )}
        {action && (
          <Button variant="ghost" size="sm" onClick={action.onClick}>
            {action.label}
          </Button>
        )}
      </div>
    )
  }

  return (
    <div className={cn("flex flex-col items-center justify-center gap-3 py-12 text-center text-muted-foreground", className)}>
      <span>{resolvedIcon}</span>
      <div className="space-y-1">
        <p className="text-base font-medium text-foreground">{title}</p>
        {description && (
          <p className="text-sm text-muted-foreground">{description}</p>
        )}
      </div>
      {action && (
        <Button variant="outline" size="sm" onClick={action.onClick}>
          {action.label}
        </Button>
      )}
    </div>
  )
}
