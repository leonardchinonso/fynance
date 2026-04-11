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

interface PortfolioHistoryProps {
  history: PortfolioHistoryRow[]
}

export function PortfolioHistory({ history }: PortfolioHistoryProps) {
  // Filter out months where total wealth is 0 (no data yet)
  const filtered = history.filter((row) => parseFloat(row.total_wealth) > 0)

  const chartData = filtered.map((row) => ({
    month: formatMonthShort(row.month),
    Available: parseFloat(row.available_wealth),
    Unavailable: parseFloat(row.unavailable_wealth),
    Total: parseFloat(row.total_wealth),
  }))

  return (
    <div className="space-y-6">
      {/* Line chart */}
      <div className="rounded-lg border p-4">
        <h3 className="mb-2 text-sm font-medium text-muted-foreground">
          Portfolio History
        </h3>
        <StyledLineChart
          data={chartData}
          index="month"
          categories={["Total", "Available", "Unavailable"]}
          colors={["#22c55e", "#3b82f6", "#f97316"]}
          height={340}
          curved
        />
      </div>

      {/* History table */}
      <div className="overflow-x-auto rounded-lg border">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Month</TableHead>
              <TableHead className="text-right">Available Wealth</TableHead>
              <TableHead className="text-right">Unavailable Wealth</TableHead>
              <TableHead className="text-right">Total Wealth</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {filtered.map((row) => (
              <TableRow key={row.month}>
                <TableCell>{formatMonthShort(row.month)}</TableCell>
                <TableCell className="text-right tabular-nums">
                  {formatCurrency(row.available_wealth)}
                </TableCell>
                <TableCell className="text-right tabular-nums">
                  {formatCurrency(row.unavailable_wealth)}
                </TableCell>
                <TableCell className="text-right tabular-nums font-medium">
                  {formatCurrency(row.total_wealth)}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>
    </div>
  )
}
