import { useState } from "react"
import type { SpendingGridRow, Granularity } from "@/types"
import type { RemoteData } from "@/lib/remote_data"
import { visitRemoteData } from "@/lib/remote_data"
import {
  Table, TableBody, TableCell, TableHead, TableHeader, TableRow,
} from "@/components/ui/table"
import {
  cn, formatCurrency, categoryLeaf, groupMonthsByGranularity, getMonthsForPeriod, formatPeriodKey,
} from "@/lib/utils"
import { DualAmount } from "@/components/currency"
import { usePreferredCurrency } from "@/context/preferred_currency_context"
import { SpreadsheetSkeleton } from "@/components/skeletons"
import { AuthAwareError } from "@/components/auth_aware_error"
import { ReloadingOverlay } from "@/components/reloading_overlay"
import { EmptyState } from "@/components/empty_state"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { Switch } from "@/components/ui/switch"
import { BudgetEditPopover } from "./budget_edit_popover"

const SHOW_EMPTY_KEY = "fynance-budget-show-empty"

interface BudgetSpreadsheetProps {
  data: RemoteData<SpendingGridRow[]>
  months: string[]
  granularity: Granularity
  onBudgetSaved?: () => void
}

export function BudgetSpreadsheet({ data, months, granularity, onBudgetSaved }: BudgetSpreadsheetProps) {
  return visitRemoteData(data, {
    notLoaded: () => <SpreadsheetSkeleton />,
    failed: (error) => <AuthAwareError error={error} onRetry={onBudgetSaved} />,
    hasValue: (rows) => (
      <div className="relative">
        <BudgetSpreadsheetInternal rows={rows} months={months} granularity={granularity} onBudgetSaved={onBudgetSaved} />
        <ReloadingOverlay active={data.status === "reloading"} />
      </div>
    ),
  })
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

function BudgetSpreadsheetInternal({ rows, months, granularity, onBudgetSaved }: {
  rows: SpendingGridRow[]; months: string[]; granularity: Granularity; onBudgetSaved?: () => void
}) {
  const preferredCurrency = usePreferredCurrency()
  const [showEmpty, setShowEmpty] = useState<boolean>(() => {
    try { return localStorage.getItem(SHOW_EMPTY_KEY) === "true" } catch { return false }
  })
  const toggleShowEmpty = (v: boolean) => {
    setShowEmpty(v)
    try { localStorage.setItem(SHOW_EMPTY_KEY, String(v)) } catch { /* ignore */ }
  }
  if (rows.length === 0) return <EmptyState />

  const periods = groupMonthsByGranularity(months, granularity)

  function getPeriodBudget(monthlyBudget: string | null, periodKey: string): string | null {
    if (!monthlyBudget) return null
    const periodMonths = getMonthsForPeriod(months, periodKey, granularity)
    return (parseFloat(monthlyBudget) * periodMonths.length).toFixed(2)
  }

  function getPeriodValue(row: SpendingGridRow, periodKey: string): string | null {
    const periodMonths = getMonthsForPeriod(months, periodKey, granularity)
    let total = 0
    let hasData = false
    for (const m of periodMonths) {
      const val = row.periods[m]
      if (val != null) { total += parseFloat(val); hasData = true }
    }
    return hasData ? total.toFixed(2) : null
  }

  const isEmptyRow = (row: SpendingGridRow): boolean => {
    if (row.budget != null && parseFloat(row.budget) !== 0) return false
    for (const p of periods) {
      const v = getPeriodValue(row, p)
      if (v != null && parseFloat(v) !== 0) return false
    }
    return true
  }
  const visibleRows = showEmpty ? rows : rows.filter((r) => !isEmptyRow(r))

  const sections = ["Income", "Bills", "Spending", "Irregular", "Transfers"]
  const grouped = new Map<string, SpendingGridRow[]>()
  for (const s of sections) grouped.set(s, [])
  for (const row of visibleRows) {
    const arr = grouped.get(row.section)
    if (arr) arr.push(row)
  }

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-end">
        <label className="flex cursor-pointer select-none items-center gap-2 text-xs text-muted-foreground">
          <Switch size="sm" checked={showEmpty} onCheckedChange={toggleShowEmpty} />
          Show empty categories
        </label>
      </div>
      {visibleRows.length === 0 ? (
        <div className="rounded-lg border p-6 text-center text-sm text-muted-foreground">
          No categories with budget or spend in this range.
        </div>
      ) : (
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
                preferredCurrency={preferredCurrency}
                onBudgetSaved={onBudgetSaved}
              />
            )
          })}
        </TableBody>
          </Table>
        </div>
      )}
    </div>
  )
}

function SectionBlock({
  section, rows, periods, months, granularity, getPeriodValue, getPeriodBudget, preferredCurrency, onBudgetSaved,
}: {
  section: string
  rows: SpendingGridRow[]
  periods: string[]
  months: string[]
  granularity: Granularity
  getPeriodValue: (row: SpendingGridRow, periodKey: string) => string | null
  getPeriodBudget: (budget: string | null, periodKey: string) => string | null
  preferredCurrency: string
  onBudgetSaved?: () => void
}) {
  const totals: Record<string, number | null> = {}
  for (const p of periods) totals[p] = null
  for (const row of rows) {
    for (const p of periods) {
      const val = getPeriodValue(row, p)
      if (val !== null) totals[p] = (totals[p] ?? 0) + Math.abs(parseFloat(val))
    }
  }
  const periodsWithTotals = Object.values(totals).filter((v) => v !== null) as number[]
  const totalAvg = periodsWithTotals.length > 0
    ? periodsWithTotals.reduce((s, v) => s + v, 0) / periodsWithTotals.length
    : 0

  return (
    <>
      <TableRow className="bg-muted/50">
        <TableCell colSpan={periods.length + 3} className="sticky left-0 font-semibold text-xs uppercase tracking-wider">
          {section}
        </TableCell>
      </TableRow>
      {rows.map((row) => {
        const rowValues = periods.map((p) => getPeriodValue(row, p))
        const nonNullValues = rowValues.filter((v) => v !== null) as string[]
        const rowAvg = nonNullValues.length > 0
          ? nonNullValues.reduce((s, v) => s + Math.abs(parseFloat(v)), 0) / nonNullValues.length
          : null

        return (
          <TableRow key={row.category}>
            <TableCell className="sticky left-0 bg-background text-sm z-10">
              {categoryLeaf(row.category)}
            </TableCell>
            {periods.map((p, i) => {
              const val = rowValues[i]
              if (val === null) return (
                <TableCell key={p} className="text-right text-sm text-muted-foreground/30">-</TableCell>
              )
              const periodBudget = getPeriodBudget(row.budget, p)
              // periods_display is keyed by month; for aggregated periods find the first constituent month with a display entry
              const display = row.periods_display ?? {}
              const periodDisplay = display[p]
                ?? getMonthsForPeriod(months, p, granularity).map(m => display[m]).find(d => d != null)
                ?? null
              return (
                <TableCell key={p} className={cn("text-right text-sm", row.section !== "Income" && cellColor(val, periodBudget))}>
                  <DualAmount value={Math.abs(parseFloat(val)).toFixed(2)} preferredCurrency={preferredCurrency} display={periodDisplay} secondaryFirst />
                </TableCell>
              )
            })}
            <TableCell className="text-right text-sm font-medium">
              {rowAvg !== null ? (
                <Tooltip>
                  <TooltipTrigger className="cursor-default underline decoration-dotted decoration-muted-foreground/40 underline-offset-2">
                    <DualAmount value={rowAvg.toFixed(2)} preferredCurrency={preferredCurrency} display={row.average_display} secondaryFirst />
                  </TooltipTrigger>
                  <TooltipContent
                    side="left"
                    className="max-w-xs bg-popover text-popover-foreground ring-1 ring-foreground/10 px-3 py-2"
                    arrowClassName="bg-popover fill-popover"
                  >
                    <div className="space-y-1 text-xs">
                      <p className="text-[10px] font-medium text-muted-foreground">{categoryLeaf(row.category)} — spend by period</p>
                      <table className="w-full tabular-nums">
                        <tbody>
                          {periods.map((p, i) => {
                            const v = rowValues[i]
                            return (
                              <tr key={p}>
                                <td className="pr-3 text-left text-[10px] text-muted-foreground">{formatPeriodKey(p, granularity)}</td>
                                <td className="text-right">{v !== null ? formatCurrency(Math.abs(parseFloat(v)).toFixed(2), preferredCurrency) : "—"}</td>
                              </tr>
                            )
                          })}
                        </tbody>
                      </table>
                    </div>
                  </TooltipContent>
                </Tooltip>
              ) : "-"}
            </TableCell>
            <TableCell className="text-right text-sm tabular-nums">
              <BudgetEditPopover
                category={row.category}
                category_id={row.category_id}
                currentBudget={row.budget ?? null}
                onSaved={onBudgetSaved}
              />
            </TableCell>
          </TableRow>
        )
      })}
      <TableRow className="border-t-2">
        <TableCell className="sticky left-0 bg-background font-medium text-sm z-10">
          Total {section}
        </TableCell>
        {periods.map((p) => (
          <TableCell key={p} className="text-right text-sm tabular-nums font-medium">
            {totals[p] !== null ? formatCurrency(totals[p]!.toFixed(2), preferredCurrency) : <span className="text-muted-foreground/30">-</span>}
          </TableCell>
        ))}
        <TableCell className="text-right text-sm tabular-nums font-medium">
          {formatCurrency(totalAvg.toFixed(2), preferredCurrency)}
        </TableCell>
        <TableCell />
      </TableRow>
    </>
  )
}
