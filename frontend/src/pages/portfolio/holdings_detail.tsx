import type { Holding } from "@/types"
import { visitRemoteData } from "@/lib/remote_data"
import { useHoldings } from "@/hooks/data"
import {
  Sheet, SheetContent, SheetHeader, SheetTitle,
} from "@/components/ui/sheet"
import {
  Table, TableBody, TableCell, TableHead, TableHeader, TableRow,
} from "@/components/ui/table"
import { Badge } from "@/components/ui/badge"
import { Currency } from "@/components/currency"
import { EmptyState } from "@/components/empty_state"
import { NonIdealState } from "@/components/non_ideal_state"
import { LoadingSpinner } from "@/components/loading_spinner"

interface HoldingsDetailProps {
  accountId: string | null
  accountName: string
  onClose: () => void
}

export function HoldingsDetail({ accountId, accountName, onClose }: HoldingsDetailProps) {
  const holdingsData = useHoldings(accountId)

  const content = visitRemoteData(holdingsData, {
    notLoaded: () => <LoadingSpinner />,
    failed: (error) => <NonIdealState title="Could not load holdings" description={error} className="mt-4" />,
    hasValue: (holdings) =>
      holdings.length === 0 ? (
        <div className="mt-4">
          <EmptyState title="No holdings on file" message="This account doesn't have any recorded positions yet." />
        </div>
      ) : (
        <HoldingsTable holdings={holdings} />
      ),
  })

  return (
    <Sheet open={!!accountId} onOpenChange={() => onClose()}>
      <SheetContent className="w-full sm:max-w-2xl overflow-y-auto">
        <SheetHeader>
          <SheetTitle>{accountName} Holdings</SheetTitle>
        </SheetHeader>
        {content}
      </SheetContent>
    </Sheet>
  )
}

function HoldingsTable({ holdings }: { holdings: Holding[] }) {
  const totalValue = holdings.reduce((s, h) => s + parseFloat(h.value), 0)
  return (
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
          {[...holdings].sort((a, b) => parseFloat(b.value) - parseFloat(a.value)).map((h) => (
            <TableRow key={`${h.account_id}-${h.symbol}`}>
              <TableCell className="font-medium">{h.symbol}</TableCell>
              <TableCell className="text-sm">{h.name}</TableCell>
              <TableCell>
                <Badge variant="outline" className="text-xs uppercase">{h.holding_type}</Badge>
              </TableCell>
              <TableCell className="text-right tabular-nums">{h.quantity}</TableCell>
              <TableCell className="text-right tabular-nums">
                {h.price_per_unit ? <Currency amount={h.price_per_unit} colorize={false} /> : "-"}
              </TableCell>
              <TableCell className="text-right tabular-nums font-medium">
                <Currency amount={h.value} colorize={false} />
              </TableCell>
              <TableCell className="text-right tabular-nums text-muted-foreground">
                {totalValue > 0 ? ((parseFloat(h.value) / totalValue) * 100).toFixed(1) : "0"}%
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
      <div className="mt-3 text-right text-sm font-medium">
        Total: <Currency amount={totalValue.toFixed(2)} colorize={false} />
      </div>
    </div>
  )
}
