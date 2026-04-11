import type { PortfolioHistoryRow } from "@/types"
import { LineChart } from "@tremor/react"
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
  const chartData = history.map((row) => ({
    month: formatMonthShort(row.month),
    Available: parseFloat(row.available_wealth),
    Unavailable: parseFloat(row.unavailable_wealth),
    Total: parseFloat(row.total_wealth),
  }))

  return (
    <div className="space-y-6">
      {/* Line chart */}
      <div className="rounded-lg border p-4">
        <h3 className="mb-4 text-sm font-medium text-muted-foreground">
          Portfolio History
        </h3>
        <LineChart
          data={chartData}
          index="month"
          categories={["Available", "Unavailable", "Total"]}
          valueFormatter={(v) => formatCurrency(v.toString())}
          className="h-80"
          colors={["blue", "orange", "green"]}
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
            {history.map((row) => (
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
