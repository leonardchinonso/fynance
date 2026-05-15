import type { BudgetRow } from "@/types"
import { Progress } from "@/components/ui/progress"
import { MoneyDisplay, DualAmount } from "@/components/currency"
import { cn, formatCurrency } from "@/lib/utils"
import { getBudgetProgressClass, getBudgetStatusClass } from "@/lib/colors"
import { usePreferredCurrency } from "@/context/preferred_currency_context"

interface BudgetProgressProps {
  rows: BudgetRow[]
}

export function BudgetProgress({ rows }: BudgetProgressProps) {
  const preferredCurrency = usePreferredCurrency()
  return (
    <div className="space-y-3">
      {rows.map((row) => {
        const pct = row.percent ?? 0
        const budgeted = row.budgeted ?? "0"
        return (
          <div
            key={row.category}
            className="rounded-lg border p-4"
          >
            <div className="mb-2 flex items-center justify-between">
              <span className="text-sm font-medium">{row.category}</span>
              <span className={cn("text-sm font-medium", getBudgetStatusClass(pct))}>
                {pct}%
              </span>
            </div>
            <Progress
              value={Math.min(pct, 100)}
              className={cn("h-2", getBudgetProgressClass(pct))}
            />
            <div className="mt-1.5 flex justify-between text-xs text-muted-foreground">
              <span className="flex items-baseline gap-1 flex-wrap">
                <DualAmount value={row.actual} preferredCurrency={preferredCurrency} display={row.actual_display} />
                <span>/</span>
                <MoneyDisplay amount={budgeted} currency={preferredCurrency} colorize={false} />
              </span>
              <span>
                {parseFloat(budgeted) - parseFloat(row.actual) > 0
                  ? `${formatCurrency((parseFloat(budgeted) - parseFloat(row.actual)).toFixed(2), preferredCurrency)} remaining`
                  : `${formatCurrency((parseFloat(row.actual) - parseFloat(budgeted)).toFixed(2), preferredCurrency)} over budget`}
              </span>
            </div>
          </div>
        )
      })}
    </div>
  )
}
