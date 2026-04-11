import { useState } from "react"
import type { PortfolioHistoryRow } from "@/types"
import { StyledLineChart } from "@/components/charts"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { formatCurrency, formatMonthShort } from "@/lib/utils"
import { cn } from "@/lib/utils"

interface PortfolioHistoryProps {
  history: PortfolioHistoryRow[]
}

export function PortfolioHistory({ history }: PortfolioHistoryProps) {
  const [hoveredRowIndex, setHoveredRowIndex] = useState<number | null>(null)
  const [brushRange, setBrushRange] = useState<{ start: number; end: number } | null>(null)

  // Filter out months where total wealth is 0 (no data yet)
  const filtered = history.filter((row) => parseFloat(row.total_wealth) > 0)

  // Apply brush range filter
  const displayed = brushRange
    ? filtered.slice(brushRange.start, brushRange.end + 1)
    : filtered

  const chartData = filtered.map((row) => ({
    month: formatMonthShort(row.month),
    Available: parseFloat(row.available_wealth),
    Unavailable: parseFloat(row.unavailable_wealth),
    Total: parseFloat(row.total_wealth),
  }))

  return (
    <div className="space-y-6">
      {/* Line chart with brush */}
      <div className="rounded-lg border p-4">
        <div className="mb-2 flex items-center justify-between">
          <h3 className="text-sm font-medium text-muted-foreground">
            Portfolio History
          </h3>
          {brushRange && (
            <button
              className="text-xs text-muted-foreground hover:text-foreground transition-colors"
              onClick={() => setBrushRange(null)}
            >
              Reset zoom
            </button>
          )}
        </div>
        <StyledLineChart
          data={chartData}
          index="month"
          categories={["Total", "Available", "Unavailable"]}
          colors={["#22c55e", "#3b82f6", "#f97316"]}
          height={340}
          curved
          showBrush
          highlightIndex={hoveredRowIndex}
          onBrushChange={(start, end) => setBrushRange({ start, end })}
        />
        <p className="mt-2 text-xs text-muted-foreground">
          Drag the handles below the chart to zoom into a date range. Hover over table rows to highlight on the chart.
        </p>
      </div>

      {/* History table with hover sync */}
      <div className="overflow-x-auto rounded-lg border">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Month</TableHead>
              <TableHead className="text-right">Available Wealth</TableHead>
              <TableHead className="text-right">Unavailable Wealth</TableHead>
              <TableHead className="text-right">Total Wealth</TableHead>
              <TableHead className="text-right">Change</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {displayed.map((row, i) => {
              // Find the index in the full chartData array for highlight sync
              const chartIndex = filtered.findIndex((r) => r.month === row.month)
              const prevRow = i > 0 ? displayed[i - 1] : null
              const change = prevRow
                ? parseFloat(row.total_wealth) - parseFloat(prevRow.total_wealth)
                : null

              return (
                <TableRow
                  key={row.month}
                  className={cn(
                    "cursor-pointer transition-colors",
                    hoveredRowIndex === chartIndex && "bg-muted/50"
                  )}
                  onMouseEnter={() => setHoveredRowIndex(chartIndex)}
                  onMouseLeave={() => setHoveredRowIndex(null)}
                >
                  <TableCell className="font-medium">
                    {formatMonthShort(row.month)}
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
