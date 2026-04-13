import { useState } from "react"
import type { PortfolioHistoryRow, Granularity } from "@/types"
import { StyledLineChart } from "@/components/charts"
import { EmptyState } from "@/components/empty_state"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { formatCurrency, formatMonth, getQuarter, getYear } from "@/lib/utils"
import { cn } from "@/lib/utils"

interface PortfolioHistoryProps {
  history: PortfolioHistoryRow[]
  granularity: Granularity
}

function formatPeriodLabel(key: string, granularity: Granularity): string {
  if (granularity === "monthly") return formatMonth(key)
  return key // Q1 2024 or 2024 are already readable
}

function aggregateHistory(
  history: PortfolioHistoryRow[],
  granularity: Granularity
): PortfolioHistoryRow[] {
  if (granularity === "monthly") return history

  const keyFn = granularity === "quarterly" ? getQuarter : getYear
  const groups = new Map<
    string,
    { available: number; unavailable: number; count: number }
  >()
  const orderedKeys: string[] = []

  for (const row of history) {
    const key = keyFn(row.month)
    if (!groups.has(key)) {
      groups.set(key, { available: 0, unavailable: 0, count: 0 })
      orderedKeys.push(key)
    }
    const g = groups.get(key)!
    g.available = parseFloat(row.available_wealth)
    g.unavailable = parseFloat(row.unavailable_wealth)
    g.count++
  }

  return orderedKeys.map((key) => {
    const g = groups.get(key)!
    return {
      month: key,
      available_wealth: g.available.toFixed(2),
      unavailable_wealth: g.unavailable.toFixed(2),
      total_wealth: (g.available + g.unavailable).toFixed(2),
    }
  })
}

export function PortfolioHistory({ history, granularity }: PortfolioHistoryProps) {
  const [hoveredIndex, setHoveredIndex] = useState<number | null>(null)

  const filtered = history.filter((row) => parseFloat(row.total_wealth) > 0)
  const aggregated = aggregateHistory(filtered, granularity)

  if (aggregated.length === 0) {
    return <EmptyState />
  }

  const chartData = aggregated.map((row) => ({
    period: formatPeriodLabel(row.month, granularity),
    Available: parseFloat(row.available_wealth),
    Unavailable: parseFloat(row.unavailable_wealth),
    Total: parseFloat(row.total_wealth),
  }))

  const periodLabel =
    granularity === "monthly"
      ? "Month"
      : granularity === "quarterly"
        ? "Quarter"
        : "Year"

  return (
    <div className="space-y-6">
      <div className="rounded-lg border p-4">
        <div className="mb-2 flex items-center justify-between">
          <h3 className="text-sm font-medium text-muted-foreground">
            Portfolio History
          </h3>
        </div>
        <StyledLineChart
          data={chartData}
          index="period"
          categories={["Total", "Available", "Unavailable"]}
          colors={["#22c55e", "#3b82f6", "#f97316"]}
          height={340}
          curved
          highlightIndex={hoveredIndex}
          onActiveIndexChange={setHoveredIndex}
        />
      </div>

      <div className="overflow-x-auto rounded-lg border">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>{periodLabel}</TableHead>
              <TableHead className="text-right">Available Wealth</TableHead>
              <TableHead className="text-right">Unavailable Wealth</TableHead>
              <TableHead className="text-right">Total Wealth</TableHead>
              <TableHead className="text-right">Change</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {aggregated.map((row, i) => {
              const prevRow = i > 0 ? aggregated[i - 1] : null
              const change = prevRow
                ? parseFloat(row.total_wealth) - parseFloat(prevRow.total_wealth)
                : null

              return (
                <TableRow
                  key={row.month}
                  className={cn(
                    "cursor-pointer transition-colors",
                    hoveredIndex === i && "bg-muted/50"
                  )}
                  onMouseEnter={() => setHoveredIndex(i)}
                  onMouseLeave={() => setHoveredIndex(null)}
                >
                  <TableCell className="font-medium">
                    {formatPeriodLabel(row.month, granularity)}
                  </TableCell>
                  <TableCell className="text-right tabular-nums">
                    {formatCurrency(row.available_wealth)}
                  </TableCell>
                  <TableCell className="text-right tabular-nums">
                    {formatCurrency(row.unavailable_wealth)}
                  </TableCell>
                  <TableCell className="text-right tabular-nums font-medium">
                    {formatCurrency(row.total_wealth)}
                  </TableCell>
                  <TableCell className="text-right tabular-nums">
                    {change !== null ? (
                      <span
                        className={cn(
                          "text-sm",
                          change >= 0 ? "text-green-500" : "text-red-500"
                        )}
                      >
                        {change >= 0 ? "+" : ""}
                        {formatCurrency(change.toFixed(2))}
                      </span>
                    ) : (
                      <span className="text-muted-foreground">-</span>
                    )}
                  </TableCell>
                </TableRow>
              )
            })}
          </TableBody>
        </Table>
      </div>
    </div>
  )
}
