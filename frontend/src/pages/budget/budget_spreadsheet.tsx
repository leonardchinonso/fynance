import type { SpendingGridRow, Granularity } from "@/types"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import {
  cn,
  formatCurrency,
  groupMonthsByGranularity,
  getMonthsForPeriod,
  formatPeriodKey,
} from "@/lib/utils"

interface BudgetSpreadsheetProps {
  rows: SpendingGridRow[]
  months: string[]
  granularity: Granularity
}

function cellColor(value: string, budget: string | null): string {
  if (!budget) return ""
  const v = Math.abs(parseFloat(value))
  const b = parseFloat(budget)
  if (b === 0) return ""
  const pct = (v / b) * 100
  if (pct > 110) return "bg-red-500/15 text-red-600 dark:text-red-400"
  if (pct >= 80) return "bg-amber-500/15 text-amber-600 dark:text-amber-400"
  if (v > 0) return "bg-green-500/10 text-green-600 dark:text-green-400"
  return ""
}

export function BudgetSpreadsheet({ rows, months, granularity }: BudgetSpreadsheetProps) {
  const periods = groupMonthsByGranularity(months, granularity)

  // For quarterly/yearly budgets, multiply the monthly budget by the number of months in the period
  function getPeriodBudget(monthlyBudget: string | null, periodKey: string): string | null {
    if (!monthlyBudget) return null
    const periodMonths = getMonthsForPeriod(months, periodKey, granularity)
    const multiplier = periodMonths.length
    return (parseFloat(monthlyBudget) * multiplier).toFixed(2)
  }

  // Aggregate a row's values for a period
  function getPeriodValue(row: SpendingGridRow, periodKey: string): string | null {
    const periodMonths = getMonthsForPeriod(months, periodKey, granularity)
    let total = 0
    let hasData = false
    for (const m of periodMonths) {
      const val = row.periods[m]
      if (val !== null) {
        total += parseFloat(val)
        hasData = true
      }
    }
    return hasData ? total.toFixed(2) : null
  }

  // Group rows by section
  const sections = ["Income", "Bills", "Spending", "Irregular", "Transfers"]
  const grouped = new Map<string, SpendingGridRow[]>()
  for (const s of sections) grouped.set(s, [])
  for (const row of rows) {
    const arr = grouped.get(row.section)
    if (arr) arr.push(row)
  }

  return (
    <div className="overflow-x-auto rounded-lg border">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead className="sticky left-0 bg-background z-10">Category</TableHead>
            {periods.map((p) => (
              <TableHead key={p} className="text-right whitespace-nowrap">
                {formatPeriodKey(p, granularity)}
              </TableHead>
            ))}
            <TableHead className="text-right">Average</TableHead>
            <TableHead className="text-right">Budget</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {sections.map((section) => {
            const sectionRows = grouped.get(section) ?? []
            if (sectionRows.length === 0) return null
            return (
              <SectionBlock
                key={section}
                section={section}
                rows={sectionRows}
                periods={periods}
                months={months}
                granularity={granularity}
                getPeriodValue={getPeriodValue}
                getPeriodBudget={getPeriodBudget}
              />
            )
          })}
        </TableBody>
      </Table>
    </div>
  )
}

function SectionBlock({
  section,
  rows,
  periods,
  getPeriodValue,
  getPeriodBudget,
}: {
  section: string
  rows: SpendingGridRow[]
  periods: string[]
  months: string[]
  granularity: Granularity
  getPeriodValue: (row: SpendingGridRow, periodKey: string) => string | null
  getPeriodBudget: (budget: string | null, periodKey: string) => string | null
}) {
  // Compute section totals per period
  const totals: Record<string, number | null> = {}
  for (const p of periods) totals[p] = null
  for (const row of rows) {
    for (const p of periods) {
      const val = getPeriodValue(row, p)
      if (val !== null) {
        totals[p] = (totals[p] ?? 0) + Math.abs(parseFloat(val))
      }
    }
  }
  const periodsWithTotals = Object.values(totals).filter((v) => v !== null) as number[]
  const totalAvg =
    periodsWithTotals.length > 0
      ? periodsWithTotals.reduce((s, v) => s + v, 0) / periodsWithTotals.length
      : 0

  return (
    <>
      {/* Section header */}
      <TableRow className="bg-muted/50">
        <TableCell
          colSpan={periods.length + 3}
          className="sticky left-0 font-semibold text-xs uppercase tracking-wider"
        >
          {section}
        </TableCell>
      </TableRow>
      {/* Data rows */}
      {rows.map((row) => {
        const rowValues = periods.map((p) => getPeriodValue(row, p))
        const nonNullValues = rowValues.filter((v) => v !== null) as string[]
        const rowAvg =
          nonNullValues.length > 0
            ? nonNullValues.reduce((s, v) => s + Math.abs(parseFloat(v)), 0) / nonNullValues.length
            : null

        return (
          <TableRow key={row.category}>
            <TableCell className="sticky left-0 bg-background text-sm z-10">
              {row.category.split(": ").pop()}
            </TableCell>
            {periods.map((p, i) => {
              const val = rowValues[i]
              if (val === null) {
                return (
                  <TableCell key={p} className="text-right text-sm text-muted-foreground/30">
                    -
                  </TableCell>
                )
              }
              const periodBudget = getPeriodBudget(row.budget, p)
              return (
                <TableCell
                  key={p}
                  className={cn(
                    "text-right text-sm tabular-nums",
                    row.section !== "Income" && cellColor(val, periodBudget)
                  )}
                >
                  {formatCurrency(Math.abs(parseFloat(val)).toFixed(2))}
                </TableCell>
              )
            })}
            <TableCell className="text-right text-sm tabular-nums font-medium">
              {rowAvg !== null
                ? formatCurrency(rowAvg.toFixed(2))
                : "-"}
            </TableCell>
            <TableCell className="text-right text-sm tabular-nums">
              {row.budget ? formatCurrency(row.budget) : "-"}
            </TableCell>
          </TableRow>
        )
      })}
      {/* Section total */}
      <TableRow className="border-t-2">
        <TableCell className="sticky left-0 bg-background font-medium text-sm z-10">
          Total {section}
        </TableCell>
        {periods.map((p) => (
          <TableCell key={p} className="text-right text-sm tabular-nums font-medium">
            {totals[p] !== null ? formatCurrency(totals[p]!.toFixed(2)) : <span className="text-muted-foreground/30">-</span>}
          </TableCell>
        ))}
        <TableCell className="text-right text-sm tabular-nums font-medium">
          {formatCurrency(totalAvg.toFixed(2))}
        </TableCell>
        <TableCell />
      </TableRow>
    </>
  )
}
