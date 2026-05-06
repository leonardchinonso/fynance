import type { Currency, Holding } from "@/types"
import type { HoldingType } from "@/bindings/HoldingType"
import { visitRemoteData } from "@/lib/remote_data"
import { useHoldings, useCurrencies } from "@/hooks/data"
import {
  Sheet, SheetContent, SheetHeader, SheetTitle,
} from "@/components/ui/sheet"
import {
  Table, TableBody, TableCell, TableHead, TableHeader, TableRow,
} from "@/components/ui/table"
import { Badge } from "@/components/ui/badge"
import { MoneyDisplay } from "@/components/currency"
import { InteractivePie } from "@/components/charts"
import { EmptyState } from "@/components/empty_state"
import { AuthAwareError } from "@/components/auth_aware_error"
import { LoadingSpinner } from "@/components/loading_spinner"
import { usePreferredCurrency } from "@/context/preferred_currency_context"
import { formatCurrency } from "@/lib/utils"

// Colors and labels per holding type
const HOLDING_TYPE_COLORS: Record<HoldingType, string> = {
  stock:    "#3b82f6",
  etf:      "#22c55e",
  fund:     "#a855f7",
  bond:     "#f97316",
  crypto:   "#eab308",
  cash:     "#78716c",
  property: "#06b6d4",
  loan:     "#ef4444",
  credit:   "#ec4899",
}

const HOLDING_TYPE_LABELS: Record<HoldingType, string> = {
  stock:    "Stock",
  etf:      "ETF",
  fund:     "Fund",
  bond:     "Bond",
  crypto:   "Crypto",
  cash:     "Cash",
  property: "Property",
  loan:     "Loan",
  credit:   "Credit",
}

const PIE_COLORS = [
  "#3b82f6", "#f97316", "#22c55e", "#a855f7", "#ec4899",
  "#06b6d4", "#eab308", "#6366f1", "#14b8a6", "#ef4444",
  "#f59e0b", "#10b981",
]

// Holding types that are investments (shown in the allocation pie)
const INVESTMENT_HOLDING_TYPES: HoldingType[] = ["stock", "etf", "fund", "bond", "crypto"]

interface InvestmentsDetailProps {
  accountId: string | null
  accountName: string
  onClose: () => void
}

export function InvestmentsDetail({ accountId, accountName, onClose }: InvestmentsDetailProps) {
  const holdingsData = useHoldings(accountId)
  const [currenciesData] = useCurrencies()
  const preferredCurrency = usePreferredCurrency()

  const currencies: Currency[] =
    currenciesData.status === "succeeded" || currenciesData.status === "reloading"
      ? currenciesData.value : []

  const fxRates = new Map<string, number>()
  for (const c of currencies) fxRates.set(c.code, parseFloat(c.fx_rate))
  const toPreferred = (value: number, currency: string) =>
    value * (fxRates.get(currency) ?? 1)

  const content = visitRemoteData(holdingsData, {
    notLoaded: () => <LoadingSpinner />,
    failed: (error) => <AuthAwareError error={error} />,
    hasValue: (holdings) =>
      holdings.length === 0 ? (
        <div className="mt-4">
          <EmptyState title="No holdings on file" message="This account doesn't have any recorded positions yet." />
        </div>
      ) : (
        <HoldingsContent holdings={holdings} preferredCurrency={preferredCurrency} toPreferred={toPreferred} />
      ),
  })

  return (
    <Sheet open={!!accountId} onOpenChange={() => onClose()}>
      <SheetContent className="w-full sm:max-w-4xl overflow-y-auto px-6">
        <SheetHeader>
          <SheetTitle>{accountName} Holdings</SheetTitle>
        </SheetHeader>
        {content}
      </SheetContent>
    </Sheet>
  )
}

function HoldingsContent({
  holdings,
  preferredCurrency,
  toPreferred,
}: {
  holdings: Holding[]
  preferredCurrency: string
  toPreferred: (value: number, currency: string) => number
}) {
  const sorted = [...holdings].sort((a, b) =>
    toPreferred(parseFloat(b.value), b.currency) - toPreferred(parseFloat(a.value), a.currency)
  )

  const totalPreferred = sorted.reduce(
    (s, h) => s + toPreferred(parseFloat(h.value), h.currency), 0
  )

  const showConvertedCol = sorted.some((h) => h.currency !== preferredCurrency)

  const preferredSymbol = new Intl.NumberFormat("en-GB", { style: "currency", currency: preferredCurrency })
    .format(0).replace(/[\d.,\s]/g, "").trim()

  // Build pie data from investment-type holdings only
  const investmentPositions = sorted.filter(
    (h) => INVESTMENT_HOLDING_TYPES.includes(h.holding_type as HoldingType) && toPreferred(parseFloat(h.value), h.currency) > 0
  )
  const showPie = investmentPositions.length > 1

  const pieData = investmentPositions.map((h) => ({
    name: h.short_name ?? h.symbol,
    fullName: h.name,
    value: parseFloat(toPreferred(parseFloat(h.value), h.currency).toFixed(2)),
  }))

  return (
    <div className="mt-4 space-y-6">
      {/* Holdings table */}
      <div>
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Symbol</TableHead>
              <TableHead>Name</TableHead>
              <TableHead>Type</TableHead>
              <TableHead className="text-right">Qty</TableHead>
              <TableHead className="text-right">Price</TableHead>
              <TableHead className="text-right">Value</TableHead>
              {showConvertedCol && (
                <TableHead className="text-right">Value ({preferredSymbol})</TableHead>
              )}
              <TableHead className="text-right">%</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {sorted.map((h) => {
              const valueInPreferred = toPreferred(parseFloat(h.value), h.currency)
              const typeColor = HOLDING_TYPE_COLORS[h.holding_type as HoldingType] ?? "#78716c"
              return (
                <TableRow key={`${h.account_id}-${h.symbol}`}>
                  <TableCell className="font-medium">{h.symbol}</TableCell>
                  <TableCell className="text-sm">{h.name}</TableCell>
                  <TableCell>
                    <Badge
                      variant="outline"
                      className="text-xs"
                      style={{ borderColor: typeColor, color: typeColor }}
                    >
                      {HOLDING_TYPE_LABELS[h.holding_type as HoldingType] ?? h.holding_type}
                    </Badge>
                  </TableCell>
                  <TableCell className="text-right tabular-nums">{h.quantity}</TableCell>
                  <TableCell className="text-right tabular-nums">
                    {h.price_per_unit
                      ? <MoneyDisplay amount={h.price_per_unit} currency={h.currency} colorize={false} />
                      : "-"}
                  </TableCell>
                  <TableCell className="text-right tabular-nums font-medium">
                    <MoneyDisplay amount={h.value} currency={h.currency} colorize={false} />
                  </TableCell>
                  {showConvertedCol && (
                    <TableCell className="text-right tabular-nums text-muted-foreground">
                      <MoneyDisplay amount={valueInPreferred.toFixed(2)} currency={preferredCurrency} colorize={false} />
                    </TableCell>
                  )}
                  <TableCell className="text-right tabular-nums text-muted-foreground">
                    {totalPreferred > 0 ? ((valueInPreferred / totalPreferred) * 100).toFixed(1) : "0"}%
                  </TableCell>
                </TableRow>
              )
            })}
          </TableBody>
        </Table>
        <div className="mt-3 text-right text-sm font-medium">
          Total: <MoneyDisplay amount={totalPreferred.toFixed(2)} currency={preferredCurrency} colorize={false} />
        </div>
      </div>

      {/* Pie chart for investment accounts */}
      {showPie && (
        <div>
          <p className="mb-3 text-xs font-semibold text-muted-foreground uppercase tracking-wider">
            Allocation
          </p>
          <InteractivePie
            data={pieData}
            colors={PIE_COLORS}
            height={260}
            innerRadius={55}
            outerRadius={95}
            label={formatCurrency(totalPreferred.toFixed(2), preferredCurrency)}
          />
        </div>
      )}
    </div>
  )
}
