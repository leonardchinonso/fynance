import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { Badge } from "@/components/ui/badge"
import { formatCurrency } from "@/lib/utils"
import type { SymbolSummary } from "@/bindings/SymbolSummary"

interface CgtPerSymbolTableProps {
  rows: SymbolSummary[]
  baseCurrency: string
  perSymbolCounts: Record<string, number>
}

export function CgtPerSymbolTable({
  rows,
  baseCurrency,
  perSymbolCounts,
}: CgtPerSymbolTableProps) {
  if (rows.length === 0) return null
  return (
    <Card>
      <CardHeader>
        <CardTitle>Per-symbol breakdown</CardTitle>
      </CardHeader>
      <CardContent>
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Symbol</TableHead>
              <TableHead className="text-right">Disposals</TableHead>
              <TableHead className="text-right">Proceeds</TableHead>
              <TableHead className="text-right">Allowable costs</TableHead>
              <TableHead className="text-right">Net gain/loss</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {rows.map((r) => {
              const net = Number.parseFloat(r.net_gain_loss)
              return (
                <TableRow key={r.symbol}>
                  <TableCell className="font-medium">
                    <div className="flex items-center gap-2">
                      {r.symbol}
                      {r.original_currency !== baseCurrency && (
                        <Badge variant="outline" className="text-[10px] py-0">
                          {r.original_currency}
                        </Badge>
                      )}
                    </div>
                  </TableCell>
                  <TableCell className="text-right tabular-nums">
                    {perSymbolCounts[r.symbol] ?? 0}
                  </TableCell>
                  <TableCell className="text-right tabular-nums">
                    {formatCurrency(r.total_proceeds, baseCurrency)}
                  </TableCell>
                  <TableCell className="text-right tabular-nums">
                    {formatCurrency(r.total_allowable_costs, baseCurrency)}
                  </TableCell>
                  <TableCell
                    className={
                      "text-right tabular-nums font-medium " +
                      (net >= 0
                        ? "text-emerald-600 dark:text-emerald-400"
                        : "text-red-600 dark:text-red-400")
                    }
                  >
                    {formatCurrency(r.net_gain_loss, baseCurrency)}
                  </TableCell>
                </TableRow>
              )
            })}
          </TableBody>
        </Table>
      </CardContent>
    </Card>
  )
}
