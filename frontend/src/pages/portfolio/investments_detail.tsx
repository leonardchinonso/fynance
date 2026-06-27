import { useState } from "react"
import type { Holding, Granularity, Account } from "@/types"
import type { HoldingType } from "@/bindings/HoldingType"
import { visitRemoteData } from "@/lib/remote_data"
import { useHoldings } from "@/hooks/data"
import {
  Sheet, SheetContent, SheetHeader, SheetTitle,
} from "@/components/ui/sheet"
import {
  Table, TableBody, TableCell, TableHead, TableHeader, TableRow,
} from "@/components/ui/table"
import { Badge } from "@/components/ui/badge"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import { MoneyDisplay } from "@/components/currency"
import { InteractivePie } from "@/components/charts"
import { EmptyState } from "@/components/empty_state"
import { AuthAwareError } from "@/components/auth_aware_error"
import { LoadingSpinner } from "@/components/loading_spinner"
import { usePreferredCurrency, useCurrenciesFromContext } from "@/context/preferred_currency_context"
import { formatCurrency, formatDate } from "@/lib/utils"
import { ACCOUNT_TYPE_LABELS } from "@/lib/colors"
import { AccountHistoryChart, type HoverPeriod } from "./account_history_chart"

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

// Normalized holdings-table row, unified across live (current holdings) and
// hover (as-of a past period) modes. Fields that aren't tracked historically
// are null in hover mode and render as "-".
interface DisplayRow {
  key: string
  symbol: string
  name: string
  holdingType: string
  valueAmount: string
  valueCurrency: string
  pct: string
  quantity: string | null
  pricePerUnit: string | null
  priceCurrency: string | null
  convertedAmount: string | null
}

interface InvestmentsDetailProps {
  accountId: string | null
  account: Account | null
  start: string
  end: string
  onClose: () => void
}

export function InvestmentsDetail({ accountId, account, start, end, onClose }: InvestmentsDetailProps) {
  const holdingsData = useHoldings(accountId)
  const currencies = useCurrenciesFromContext()
  const preferredCurrency = usePreferredCurrency()

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
        <HoldingsContent
          holdings={holdings}
          accountId={accountId ?? ""}
          start={start}
          end={end}
          preferredCurrency={preferredCurrency}
          toPreferred={toPreferred}
        />
      ),
  })

  return (
    <Sheet open={!!accountId} onOpenChange={() => onClose()}>
      <SheetContent className="w-full sm:max-w-4xl overflow-y-auto px-6">
        <SheetHeader>
          <SheetTitle>{account?.name ?? ""} Holdings</SheetTitle>
        </SheetHeader>
        {account && <AccountMeta account={account} />}
        {content}
      </SheetContent>
    </Sheet>
  )
}

function AccountMeta({ account }: { account: Account }) {
  return (
    <div className="mt-4 space-y-2">
      <DetailRow label="Institution" value={account.institution} />
      <DetailRow label="Type" value={ACCOUNT_TYPE_LABELS[account.type]} />
      <DetailRow label="Currency" value={account.currency} />
      <DetailRow label="Balance" value={formatCurrency(account.balance ?? "0", account.currency)} />
      <DetailRow label="Last Updated" value={account.balance_date ? formatDate(account.balance_date) : "Never"} />
      {account.notes && <DetailRow label="Notes" value={account.notes} />}
    </div>
  )
}

function DetailRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex justify-between py-1.5 border-b border-border/50">
      <span className="text-sm text-muted-foreground">{label}</span>
      <span className="text-sm font-medium">{value}</span>
    </div>
  )
}

function HoldingsContent({
  holdings,
  accountId,
  start,
  end,
  preferredCurrency,
  toPreferred,
}: {
  holdings: Holding[]
  accountId: string
  start: string
  end: string
  preferredCurrency: string
  toPreferred: (value: number, currency: string) => number
}) {
  const [chartView, setChartView] = useState<"allocation" | "history">("allocation")
  const [granularity, setGranularity] = useState<Granularity>("monthly")
  // Period the cursor is over in the history chart; drives the as-of table view.
  const [hoverPeriod, setHoverPeriod] = useState<HoverPeriod | null>(null)

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

  // Only treat a hover as active while the history chart is the visible chart.
  const activeHover = !showPie || chartView === "history" ? hoverPeriod : null

  // The rows to render: the positions open at the hovered period (so rows are
  // added/removed as the cursor moves), or the current holdings otherwise.
  // Qty/price/converted aren't tracked historically, so they're null in hover
  // mode and render as "-".
  const displayRows: DisplayRow[] = activeHover
    ? activeHover.holdings.map((h) => ({
        key: h.symbol,
        symbol: h.symbol,
        name: h.name,
        holdingType: h.holdingType,
        valueAmount: h.value.toFixed(2),
        valueCurrency: preferredCurrency,
        pct: activeHover.total > 0 ? ((h.value / activeHover.total) * 100).toFixed(1) : "0",
        quantity: null,
        pricePerUnit: null,
        priceCurrency: null,
        convertedAmount: null,
      }))
    : sorted.map((h) => {
        const valueInPreferred = toPreferred(parseFloat(h.value), h.currency)
        return {
          key: `${h.account_id}-${h.symbol}`,
          symbol: h.symbol,
          name: h.name,
          holdingType: h.holding_type,
          valueAmount: h.value,
          valueCurrency: h.currency,
          pct: totalPreferred > 0 ? ((valueInPreferred / totalPreferred) * 100).toFixed(1) : "0",
          quantity: h.quantity,
          pricePerUnit: h.price_per_unit ?? null,
          priceCurrency: h.currency,
          convertedAmount: showConvertedCol ? valueInPreferred.toFixed(2) : null,
        }
      })

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
            {displayRows.map((r) => {
              const typeColor = HOLDING_TYPE_COLORS[r.holdingType as HoldingType] ?? "#78716c"
              return (
                <TableRow key={r.key}>
                  <TableCell className="font-medium">{r.symbol}</TableCell>
                  <TableCell className="text-sm">{r.name}</TableCell>
                  <TableCell>
                    <Badge
                      variant="outline"
                      className="text-xs"
                      style={{ borderColor: typeColor, color: typeColor }}
                    >
                      {HOLDING_TYPE_LABELS[r.holdingType as HoldingType] ?? r.holdingType}
                    </Badge>
                  </TableCell>
                  <TableCell className="text-right tabular-nums">{r.quantity ?? "-"}</TableCell>
                  <TableCell className="text-right tabular-nums">
                    {r.pricePerUnit
                      ? <MoneyDisplay amount={r.pricePerUnit} currency={r.priceCurrency!} colorize={false} />
                      : "-"}
                  </TableCell>
                  <TableCell className="text-right tabular-nums font-medium">
                    <MoneyDisplay amount={r.valueAmount} currency={r.valueCurrency} colorize={false} />
                  </TableCell>
                  {showConvertedCol && (
                    <TableCell className="text-right tabular-nums text-muted-foreground">
                      {r.convertedAmount
                        ? <MoneyDisplay amount={r.convertedAmount} currency={preferredCurrency} colorize={false} />
                        : "-"}
                    </TableCell>
                  )}
                  <TableCell className="text-right tabular-nums text-muted-foreground">
                    {r.pct}%
                  </TableCell>
                </TableRow>
              )
            })}
          </TableBody>
        </Table>
        <div className="mt-3 text-right text-sm font-medium">
          {activeHover && (
            <span className="mr-2 text-xs font-normal text-muted-foreground">as of {activeHover.label}</span>
          )}
          Total: <MoneyDisplay amount={(activeHover ? activeHover.total : totalPreferred).toFixed(2)} currency={preferredCurrency} colorize={false} />
        </div>
      </div>

      {/* Chart area: allocation pie (investment accounts) and/or value history.
          Accounts without a pie show history as the only chart option. */}
      <div className="space-y-3">
        {showPie && (
          <ToggleGroup
            value={[chartView]}
            onValueChange={(v) => { if (v && v.length) setChartView(v[0] as "allocation" | "history") }}
          >
            <ToggleGroupItem value="allocation" size="sm">Allocation</ToggleGroupItem>
            <ToggleGroupItem value="history" size="sm">History</ToggleGroupItem>
          </ToggleGroup>
        )}

        {showPie && chartView === "allocation" ? (
          <InteractivePie
            data={pieData}
            colors={PIE_COLORS}
            height={260}
            innerRadius={55}
            outerRadius={95}
            label={formatCurrency(totalPreferred.toFixed(2), preferredCurrency)}
          />
        ) : (
          <div className="space-y-3">
            <div className="flex justify-end">
              <ToggleGroup
                value={[granularity]}
                onValueChange={(v) => { if (v && v.length) setGranularity(v[0] as Granularity) }}
              >
                <ToggleGroupItem value="monthly" size="sm">Monthly</ToggleGroupItem>
                <ToggleGroupItem value="quarterly" size="sm">Quarterly</ToggleGroupItem>
                <ToggleGroupItem value="yearly" size="sm">Yearly</ToggleGroupItem>
              </ToggleGroup>
            </div>
            <AccountHistoryChart accountId={accountId} start={start} end={end} granularity={granularity} onHoverPeriod={setHoverPeriod} />
          </div>
        )}
      </div>
    </div>
  )
}
