import { cn } from "@/lib/utils"

interface Props {
  count: number
  className?: string
  /** When false (default), renders nothing on zero. */
  showZero?: boolean
}

/**
 * Small rounded pill showing a numeric count, e.g. `Recent imports [3]`.
 * Replaces the older `(N)` parens style throughout the app.
 */
export function CountBadge({ count, className, showZero = false }: Props) {
  if (!showZero && count === 0) return null
  return (
    <span
      className={cn(
        "inline-flex items-center justify-center rounded-full bg-secondary px-1.5 py-0.5 text-[10px] font-medium text-secondary-foreground min-w-[1.25rem] leading-none tabular-nums",
        className
      )}
    >
      {count}
    </span>
  )
}
