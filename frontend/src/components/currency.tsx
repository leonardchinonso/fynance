import type { DisplayCurrency } from "@/types"
import { cn, formatCurrency } from "@/lib/utils"

interface MoneyDisplayProps {
  amount: string
  currency?: string
  className?: string
  colorize?: boolean
}

export function MoneyDisplay({
  amount,
  currency = "GBP",
  className,
  colorize = true,
}: MoneyDisplayProps) {
  const num = parseFloat(amount)
  const formatted = formatCurrency(amount, currency)

  return (
    <span
      className={cn(
        colorize && num < 0 && "text-red-500",
        colorize && num > 0 && "text-green-500",
        className
      )}
    >
      {formatted}
    </span>
  )
}

/**
 * Shows a monetary value in its native currency, with an optional muted
 * preferred-currency equivalent in parentheses when the currencies differ.
 * Used wherever display_currency may be present.
 */
interface DualAmountProps {
  value: string
  preferredCurrency: string
  display?: DisplayCurrency | null
  className?: string
  /** When true, the muted secondary value appears to the LEFT of the primary.
   *  Use for right-aligned columns so the conversion doesn't push the main figure around. */
  secondaryFirst?: boolean
}

export function DualAmount({ value, preferredCurrency, display, className, secondaryFirst }: DualAmountProps) {
  const primary = display
    ? formatCurrency(display.value, display.currency)
    : formatCurrency(value, preferredCurrency)

  const secondary = display && display.currency !== preferredCurrency
    ? formatCurrency(value, preferredCurrency)
    : null

  return (
    <span className={cn("tabular-nums inline-flex items-baseline gap-1.5", className)}>
      {secondary && secondaryFirst && (
        <span className="text-[0.8em] text-muted-foreground font-normal">({secondary})</span>
      )}
      <span>{primary}</span>
      {secondary && !secondaryFirst && (
        <span className="text-[0.8em] text-muted-foreground font-normal">({secondary})</span>
      )}
    </span>
  )
}
