import { cn } from "@/lib/utils"
import { formatCurrency } from "@/lib/utils"

interface CurrencyProps {
  amount: string
  currency?: string
  className?: string
  colorize?: boolean
}

export function Currency({
  amount,
  currency = "GBP",
  className,
  colorize = true,
}: CurrencyProps) {
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
