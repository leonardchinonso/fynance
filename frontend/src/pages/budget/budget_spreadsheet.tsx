import type { SpendingGridRow } from "@/types"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { cn, formatMonthShort, formatCurrency } from "@/lib/utils"

interface BudgetSpreadsheetProps {
  rows: SpendingGridRow[]
  months: string[]
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

export function BudgetSpreadsheet({ rows, months }: BudgetSpreadsheetProps) {
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
            <TableHead className="sticky left-0 bg-background">Category</TableHead>
            {months.map((m) => (
              <TableHead key={m} className="text-right whitespace-nowrap">
                {formatMonthShort(m)}
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
                months={months}
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
  months,
}: {
  section: string
  rows: SpendingGridRow[]
  months: string[]
}) {
  // Compute section totals per month
  const totals: Record<string, number> = {}
  for (const m of months) totals[m] = 0
  for (const row of rows) {
    for (const m of months) {
      totals[m] += Math.abs(parseFloat(row.months[m] ?? "0"))
    }
  }
  const totalAvg =
    Object.values(totals).reduce((s, v) => s + v, 0) / months.length

  return (
    <>
      {/* Section header */}
      <TableRow className="bg-muted/50">
        <TableCell
          colSpan={months.length + 3}
          className="sticky left-0 font-semibold text-xs uppercase tracking-wider"
        >
          {section}
        </TableCell>
      </TableRow>
      {/* Data rows */}
      {rows.map((row) => (
        <TableRow key={row.category}>
          <TableCell className="sticky left-0 bg-background text-sm">
            {row.category.split(": ").pop()}
          </TableCell>
          {months.map((m) => {
            const val = row.months[m] ?? "0"
            return (
              <TableCell
                key={m}
                className={cn(
                  "text-right text-sm tabular-nums",
                  row.section !== "Income" && cellColor(val, row.budget)
                )}
              >
                {formatCurrency(Math.abs(parseFloat(val)).toFixed(2))}
              </TableCell>
            )
          })}
          <TableCell className="text-right text-sm tabular-nums font-medium">
            {row.average
              ? formatCurrency(Math.abs(parseFloat(row.average)).toFixed(2))
              : "-"}
          </TableCell>
          <TableCell className="text-right text-sm tabular-nums">
            {row.budget ? formatCurrency(row.budget) : "-"}
          </TableCell>
        </TableRow>
      ))}
      {/* Section total */}
      <TableRow className="border-t-2">
        <TableCell className="sticky left-0 bg-background font-medium text-sm">
          Total {section}
        </TableCell>
        {months.map((m) => (
          <TableCell key={m} className="text-right text-sm tabular-nums font-medium">
            {formatCurrency(totals[m].toFixed(2))}
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
