import type { DisplayCurrency } from "@/types"
import { cn, formatCurrency } from "@/lib/utils"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { useRedactedFlag } from "@/hooks/use_redacted_flag"

interface MoneyDisplayProps {
  amount: string
  currency?: string
  className?: string
  colorize?: boolean
  /** Preferred currency to convert to. With `fxRate`, a foreign-currency amount
   *  is dotted-underlined and reveals its converted value on hover. */
  preferredCurrency?: string
  /** Multiplier from `currency` to `preferredCurrency`. */
  fxRate?: string
}

export function MoneyDisplay({
  amount,
  currency = "GBP",
  className,
  colorize = true,
  preferredCurrency,
  fxRate,
}: MoneyDisplayProps) {
  useRedactedFlag()
  const num = parseFloat(amount)
  const formatted = formatCurrency(amount, currency)

  const colorClass = cn(
    colorize && num < 0 && "text-red-500",
    colorize && num > 0 && "text-green-500",
    className
  )

  const rate = fxRate ? parseFloat(fxRate) : NaN
  const showConverted =
    preferredCurrency != null &&
    currency !== preferredCurrency &&
    !isNaN(num) &&
    !isNaN(rate)

  if (!showConverted) return <span className={colorClass}>{formatted}</span>

  return (
    <Tooltip>
      <TooltipTrigger
        className={cn(
          "cursor-default underline decoration-dotted decoration-muted-foreground/40 underline-offset-2",
          colorClass,
        )}
      >
        {formatted}
      </TooltipTrigger>
      <TooltipContent
        side="left"
        className="bg-popover text-popover-foreground ring-1 ring-foreground/10 px-3 py-2 text-xs tabular-nums"
      >
        {formatCurrency(String(num * rate), preferredCurrency)}
      </TooltipContent>
    </Tooltip>
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
  /** Table-cell variant: show only the native value (dotted-underlined) and reveal
   *  the preferred-currency value on hover, to save horizontal space in dense tables. */
  tooltip?: boolean
}

export function DualAmount({ value, preferredCurrency, display, className, secondaryFirst, tooltip }: DualAmountProps) {
  useRedactedFlag()
  const primary = display
    ? formatCurrency(display.value, display.currency)
    : formatCurrency(value, preferredCurrency)

  const secondary = display && display.currency !== preferredCurrency
    ? formatCurrency(value, preferredCurrency)
    : null

  if (tooltip) {
    // No conversion to show (already in the preferred currency): render plainly.
    if (!secondary) return <span className={cn("tabular-nums", className)}>{primary}</span>
    return (
      <Tooltip>
        <TooltipTrigger
          className={cn(
            "tabular-nums cursor-default underline decoration-dotted decoration-muted-foreground/40 underline-offset-2",
            className,
          )}
        >
          {primary}
        </TooltipTrigger>
        <TooltipContent
          side="left"
          className="bg-popover text-popover-foreground ring-1 ring-foreground/10 px-3 py-2 text-xs tabular-nums"
        >
          {secondary}
        </TooltipContent>
      </Tooltip>
    )
  }

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
