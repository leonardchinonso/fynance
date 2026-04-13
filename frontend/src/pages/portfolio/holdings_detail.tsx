import { useState, useEffect } from "react"
import type { Holding } from "@/types"
import { api } from "@/api/client"
import {
  Sheet,
  SheetContent,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { Badge } from "@/components/ui/badge"
import { Currency } from "@/components/currency"
import { EmptyState } from "@/components/empty_state"
import { LoadingSpinner } from "@/components/loading_spinner"

interface HoldingsDetailProps {
  accountId: string | null
  accountName: string
  onClose: () => void
}

export function HoldingsDetail({
  accountId,
  accountName,
  onClose,
}: HoldingsDetailProps) {
  const [holdings, setHoldings] = useState<Holding[]>([])
  const [loading, setLoading] = useState(false)

  useEffect(() => {
    if (!accountId) return
    setLoading(true)
    api.getHoldings(accountId).then((h) => {
      setHoldings(h)
      setLoading(false)
    })
  }, [accountId])

  const totalValue = holdings.reduce((s, h) => s + parseFloat(h.value), 0)

  return (
    <Sheet open={!!accountId} onOpenChange={() => onClose()}>
      <SheetContent className="w-full sm:max-w-2xl overflow-y-auto">
        <SheetHeader>
          <SheetTitle>{accountName} Holdings</SheetTitle>
        </SheetHeader>
        {loading ? (
          <LoadingSpinner />
        ) : holdings.length === 0 ? (
          <div className="mt-4">
            <EmptyState
              title="No holdings on file"
              message="This account doesn't have any recorded positions yet."
            />
          </div>
        ) : (
          <div className="mt-4">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Symbol</TableHead>
                  <TableHead>Name</TableHead>
                  <TableHead>Type</TableHead>
                  <TableHead className="text-right">Qty</TableHead>
                  <TableHead className="text-right">Price</TableHead>
                  <TableHead className="text-right">Value</TableHead>
                  <TableHead className="text-right">%</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {holdings
                  .sort(
                    (a, b) => parseFloat(b.value) - parseFloat(a.value)
                  )
                  .map((h) => (
                    <TableRow key={`${h.account_id}-${h.symbol}`}>
                      <TableCell className="font-medium">{h.symbol}</TableCell>
                      <TableCell className="text-sm">{h.name}</TableCell>
                      <TableCell>
                        <Badge variant="outline" className="text-xs uppercase">
                          {h.holding_type}
                        </Badge>
                      </TableCell>
                      <TableCell className="text-right tabular-nums">
                        {h.quantity}
                      </TableCell>
                      <TableCell className="text-right tabular-nums">
                        {h.price_per_unit ? (
                          <Currency
                            amount={h.price_per_unit}
                            colorize={false}
                          />
                        ) : (
                          "-"
                        )}
                      </TableCell>
                      <TableCell className="text-right tabular-nums font-medium">
                        <Currency amount={h.value} colorize={false} />
                      </TableCell>
                      <TableCell className="text-right tabular-nums text-muted-foreground">
                        {totalValue > 0
                          ? (
                              (parseFloat(h.value) / totalValue) *
                              100
                            ).toFixed(1)
                          : "0"}
                        %
                      </TableCell>
                    </TableRow>
                  ))}
              </TableBody>
            </Table>
            <div className="mt-3 text-right text-sm font-medium">
              Total: <Currency amount={totalValue.toFixed(2)} colorize={false} />
            </div>
          </div>
        )}
      </SheetContent>
    </Sheet>
  )
}
