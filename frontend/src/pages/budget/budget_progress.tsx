import type { BudgetRow } from "@/types"
import { Progress } from "@/components/ui/progress"
import { Currency } from "@/components/currency"
import { cn } from "@/lib/utils"
import { getBudgetProgressClass, getBudgetStatusClass } from "@/lib/colors"

interface BudgetProgressProps {
  rows: BudgetRow[]
}

export function BudgetProgress({ rows }: BudgetProgressProps) {
  return (
    <div className="space-y-3">
      {rows.map((row) => (
        <div
          key={row.category}
          className="rounded-lg border p-4"
        >
          <div className="mb-2 flex items-center justify-between">
            <span className="text-sm font-medium">{row.category}</span>
            <span className={cn("text-sm font-medium", getBudgetStatusClass(row.percent))}>
              {row.percent}%
            </span>
          </div>
          <Progress
            value={Math.min(row.percent, 100)}
            className={cn("h-2", getBudgetProgressClass(row.percent))}
          />
          <div className="mt-1.5 flex justify-between text-xs text-muted-foreground">
            <span>
              <Currency amount={row.actual} colorize={false} /> /{" "}
              <Currency amount={row.budgeted} colorize={false} />
            </span>
            <span>
              {parseFloat(row.budgeted) - parseFloat(row.actual) > 0
                ? `${(parseFloat(row.budgeted) - parseFloat(row.actual)).toFixed(2)} remaining`
                : `${(parseFloat(row.actual) - parseFloat(row.budgeted)).toFixed(2)} over budget`}
            </span>
          </div>
        </div>
      ))}
    </div>
  )
}
