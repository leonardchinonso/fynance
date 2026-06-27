import type { Currency, Holding } from "@/types"
import type { InvestmentEvent } from "@/bindings/InvestmentEvent"
import type { S104PoolState } from "@/bindings/S104PoolState"
import type { CgtSummary } from "@/bindings/CgtSummary"
import type { RemoteData } from "@/lib/remote_data"
import { visitRemoteData } from "@/lib/remote_data"
import type { InvestmentsOverviewData } from "@/hooks/data"
import { PortfolioOverviewSkeleton } from "@/components/skeletons"
import { AuthAwareError } from "@/components/auth_aware_error"
import { ReloadingOverlay } from "@/components/reloading_overlay"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { EmptyState } from "@/components/empty_state"
import { MoneyDisplay } from "@/components/currency"
import { InteractivePie } from "@/components/charts"
import { StyledLineChart } from "@/components/charts"
import type { PieDataItem } from "@/components/charts/interactive_pie"
import {
  Table, TableBody, TableCell, TableHead, TableHeader, TableRow,
} from "@/components/ui/table"
import { TrendingUp, TrendingDown, Wallet, Receipt, Coins, Scale, Layers } from "lucide-react"
import { formatCurrency, formatMonthShort } from "@/lib/utils"

const STOCK_COLORS = [
  "#3b82f6", "#f97316", "#22c55e", "#a855f7", "#ec4899",
  "#06b6d4", "#eab308", "#6366f1", "#14b8a6", "#ef4444",
]

interface Props {
  data: RemoteData<InvestmentsOverviewData>
  /** Date range label, shown beside the realised-P/L card. */
  rangeLabel: string
  /** Selected range start/end (YYYY-MM-DD); scopes the invested time series. */
  start: string
  end: string
}

export function InvestmentsOverview({ data, rangeLabel, start, end }: Props) {
  return visitRemoteData(data, {
    notLoaded: () => <PortfolioOverviewSkeleton />,
    failed: (error) => <AuthAwareError error={error} />,
    hasValue: (value) => (
      <div className="relative">
        <InvestmentsOverviewInternal value={value} rangeLabel={rangeLabel} start={start} end={end} />
        <ReloadingOverlay active={data.status === "reloading"} />
      </div>
    ),
  })
}

function InvestmentsOverviewInternal({
  value,
  rangeLabel,
  start,
  end,
}: {
  value: InvestmentsOverviewData
  rangeLabel: string
  start: string
  end: string
}) {
  const { holdings, pools, events, currencies, realisedGains, preferredCurrency } = value

  const toPreferred = makeFxConverter(currencies)

  const currentValue = holdings.reduce(
    (sum, h) => sum + toPreferred(parseFloat(h.value), h.currency),
    0,
  )

  // Cost basis is the net invested across ALL investment accounts, derived from
  // the events ledger and FX-converted. The S104 pools (shown in the detail
  // table) deliberately exclude ISA/Pension, so they would understate the basis
  // against the all-accounts current value and inflate unrealised P/L.
  const invested = buildInvested(events, toPreferred, start.slice(0, 7), end.slice(0, 7))
  const costBasis = invested.total

  const unrealised = currentValue - costBasis
  const unrealisedPct = costBasis > 0 ? (unrealised / costBasis) * 100 : null

  const pieData = buildSymbolPie(holdings, toPreferred)
  const pieColorMap = new Map<string, string>()
  pieData.forEach((d, i) => pieColorMap.set(d.name, STOCK_COLORS[i % STOCK_COLORS.length]))

  const series = invested.series

  return (
    <div className="space-y-4">
      {/* Summary cards */}
      <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
        <SummaryCard
          icon={<Wallet className="h-4 w-4" />}
          label="Current value"
          hint="Market value of investment holdings"
        >
          <MoneyDisplay amount={currentValue.toFixed(2)} currency={preferredCurrency} colorize={false} />
        </SummaryCard>

        <SummaryCard
          icon={<Receipt className="h-4 w-4" />}
          label="Cost basis"
          hint="Net invested across all accounts"
        >
          <MoneyDisplay amount={costBasis.toFixed(2)} currency={preferredCurrency} colorize={false} />
        </SummaryCard>

        <SummaryCard
          icon={<Scale className="h-4 w-4" />}
          label="Unrealised P/L"
          hint="Current value minus cost basis"
        >
          <span className={`flex items-baseline gap-2 ${unrealised >= 0 ? "text-green-500" : "text-red-500"}`}>
            <span className="flex items-center gap-1">
              {unrealised >= 0 ? <TrendingUp className="h-5 w-5" /> : <TrendingDown className="h-5 w-5" />}
              {unrealised >= 0 ? "+" : ""}
              {formatCurrency(unrealised.toFixed(2), preferredCurrency)}
            </span>
            {unrealisedPct !== null && (
              <span className="text-sm font-medium opacity-75">
                ({unrealisedPct >= 0 ? "+" : ""}{unrealisedPct.toFixed(1)}%)
              </span>
            )}
          </span>
        </SummaryCard>

        <SummaryCard
          icon={<Coins className="h-4 w-4" />}
          label="Realised P/L"
          hint={rangeLabel}
        >
          <RealisedValue summary={realisedGains} />
        </SummaryCard>
      </div>

      {/* Allocation pie + cumulative-invested line */}
      <div className="grid gap-4 lg:grid-cols-2">
        <Card className="overflow-hidden py-0 gap-0 h-[340px]">
          <div className="flex flex-col h-full">
            <div className="pt-5 pl-5 pr-5 pb-2">
              <p className="text-sm font-medium text-muted-foreground">Allocation by symbol</p>
            </div>
            <div className="px-5 pb-5 flex-1 min-h-0 flex">
              {pieData.length > 0 ? (
                <InteractivePie
                  data={pieData}
                  colorMap={pieColorMap}
                  height={260}
                  innerRadius={55}
                  outerRadius={95}
                  label={formatCurrency(currentValue.toFixed(2), preferredCurrency)}
                  legendPosition="left"
                  className="w-full h-full"
                />
              ) : (
                <div className="flex-1 flex items-center justify-center">
                  <EmptyState
                    compact
                    icon={<Layers className="h-8 w-8" />}
                    title="No investment holdings"
                    message="Import holdings for an investment account to see your allocation."
                  />
                </div>
              )}
            </div>
          </div>
        </Card>

        <Card className="overflow-hidden h-[340px]">
          <CardHeader className="pb-2">
            <CardTitle className="text-sm font-medium text-muted-foreground">Cumulative invested</CardTitle>
          </CardHeader>
          <CardContent className="h-[calc(100%-3rem)]">
            {series.length > 0 ? (
              <StyledLineChart
                data={series}
                index="period"
                categories={["Invested"]}
                colors={["#a855f7"]}
                height={250}
                curved
                showLegend={false}
              />
            ) : (
              <div className="h-full flex items-center justify-center">
                <EmptyState
                  compact
                  icon={<TrendingUp className="h-8 w-8" />}
                  title="No investment events"
                  message="Record buy or vest events to chart your invested capital over time."
                />
              </div>
            )}
          </CardContent>
        </Card>
      </div>

      {/* S104 pools detail */}
      <PoolsTable pools={pools} />
    </div>
  )
}

function SummaryCard({
  icon,
  label,
  hint,
  children,
}: {
  icon: React.ReactNode
  label: string
  hint?: string
  children: React.ReactNode
}) {
  return (
    <Card>
      <CardHeader className="pb-2">
        <CardTitle className="text-sm font-medium text-muted-foreground flex items-center gap-2">
          {icon}
          {label}
        </CardTitle>
      </CardHeader>
      <CardContent>
        <div className="text-2xl font-bold tabular-nums">{children}</div>
        {hint && <p className="mt-1 text-xs text-muted-foreground">{hint}</p>}
      </CardContent>
    </Card>
  )
}

function RealisedValue({ summary }: { summary: CgtSummary | null }) {
  if (!summary) {
    return <span className="text-sm font-normal text-muted-foreground">-</span>
  }
  const net = parseFloat(summary.net_gain_loss)
  return (
    <span className={`flex items-center gap-1 ${net >= 0 ? "text-green-500" : "text-red-500"}`}>
      {net >= 0 ? <TrendingUp className="h-5 w-5" /> : <TrendingDown className="h-5 w-5" />}
      {net >= 0 ? "+" : ""}
      {formatCurrency(summary.net_gain_loss, summary.base_currency)}
    </span>
  )
}

function PoolsTable({ pools }: { pools: S104PoolState[] }) {
  const open = pools.filter((p) => parseFloat(p.current_shares) > 0)
  if (open.length === 0) {
    return (
      <EmptyState
        icon={<Layers className="h-8 w-8" />}
        title="No open pools"
        message="Once you record buy or vest events, their cost-basis pools will appear here."
      />
    )
  }
  return (
    <Card>
      <CardHeader className="pb-2">
        <CardTitle className="text-sm font-medium flex items-center gap-2">
          <Layers className="h-4 w-4" />
          Cost-basis pools (Section 104)
        </CardTitle>
        <p className="text-xs text-muted-foreground">
          Average-cost pools per symbol, derived from your investment events. This is your CGT cost basis, not market value.
        </p>
      </CardHeader>
      <CardContent className="p-0">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Symbol</TableHead>
              <TableHead className="text-right">Current shares</TableHead>
              <TableHead className="text-right">Average cost/share</TableHead>
              <TableHead className="text-right">Total allowable cost</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {open.map((p) => (
              <TableRow key={p.symbol}>
                <TableCell className="font-medium">{p.symbol}</TableCell>
                <TableCell className="text-right tabular-nums">{fmtShares(p.current_shares)}</TableCell>
                <TableCell className="text-right tabular-nums">{formatCurrency(p.average_cost_per_share)}</TableCell>
                <TableCell className="text-right tabular-nums">{formatCurrency(p.total_allowable_expenditure)}</TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </CardContent>
    </Card>
  )
}

function makeFxConverter(currencies: Currency[]): (value: number, currency: string) => number {
  const fxRates = new Map<string, number>()
  for (const c of currencies) fxRates.set(c.code, parseFloat(c.fx_rate))
  return (value, currency) => value * (fxRates.get(currency) ?? 1)
}

function buildSymbolPie(
  holdings: Holding[],
  toPreferred: (value: number, currency: string) => number,
): PieDataItem[] {
  const byName = new Map<string, { value: number; fullName: string }>()
  for (const h of holdings) {
    const converted = toPreferred(parseFloat(h.value), h.currency)
    if (converted <= 0) continue
    const key = h.short_name ?? h.symbol
    const existing = byName.get(key)
    if (existing) existing.value += converted
    else byName.set(key, { value: converted, fullName: h.name })
  }
  return Array.from(byName.entries())
    .map(([name, { value, fullName }]) => ({ name, fullName, value: parseFloat(value.toFixed(2)) }))
    .sort((a, b) => b.value - a.value)
}

/**
 * Cumulative net invested over time, bucketed by month.
 *
 * Buy/vest events add their gross cost (`quantity * price + fee`). Sells reduce
 * the running cost basis by the proportional average cost of the shares sold,
 * tracked per symbol so a sale of part of a pool removes only its share of cost.
 * Returns month-bucketed running totals suitable for `StyledLineChart`.
 */
function buildInvested(
  events: InvestmentEvent[],
  toPreferred: (value: number, currency: string) => number,
  startMonth: string,
  endMonth: string,
): { series: { period: string; Invested: number }[]; total: number } {
  const ordered = [...events].sort((a, b) => a.date.localeCompare(b.date))

  const pools = new Map<string, { shares: number; cost: number }>()
  let running = 0
  // Running net-invested value at the end of each month that has an event.
  const monthly = new Map<string, number>()

  for (const e of ordered) {
    const qty = parseFloat(e.quantity)
    const price = parseFloat(e.price_per_share)
    const fee = e.fee ? toPreferred(parseFloat(e.fee), e.fee_currency ?? e.currency) : 0
    const gross = toPreferred(qty * price, e.currency) + fee
    const pool = pools.get(e.symbol) ?? { shares: 0, cost: 0 }

    if (e.event_type === "buy" || e.event_type === "vest" || e.event_type === "transfer") {
      pool.shares += qty
      pool.cost += gross
      running += gross
    } else if (e.event_type === "sell" || e.event_type === "withhold") {
      const removed = pool.shares > 0 ? Math.min(qty, pool.shares) : 0
      const avg = pool.shares > 0 ? pool.cost / pool.shares : 0
      const removedCost = removed * avg
      pool.shares -= removed
      pool.cost -= removedCost
      running -= removedCost
    }
    pools.set(e.symbol, pool)
    monthly.set(e.date.slice(0, 7), parseFloat(running.toFixed(2)))
  }

  // Forward-fill across the selected month range so the line spans the chosen
  // period and starts at the basis already accumulated before it.
  const eventMonths = [...monthly.keys()].sort()
  let idx = 0
  let last = 0
  const series: { period: string; Invested: number }[] = []
  for (const m of monthRange(startMonth, endMonth)) {
    while (idx < eventMonths.length && eventMonths[idx] <= m) {
      last = monthly.get(eventMonths[idx])!
      idx++
    }
    series.push({ period: formatMonthShort(m), Invested: last })
  }

  return { series, total: parseFloat(running.toFixed(2)) }
}

/** Inclusive list of `YYYY-MM` strings from startMonth to endMonth. */
function monthRange(startMonth: string, endMonth: string): string[] {
  const [sy, sm] = startMonth.split("-").map(Number)
  const [ey, em] = endMonth.split("-").map(Number)
  const out: string[] = []
  let y = sy
  let mo = sm
  let guard = 0
  while ((y < ey || (y === ey && mo <= em)) && guard < 600) {
    out.push(`${y}-${String(mo).padStart(2, "0")}`)
    mo++
    if (mo > 12) { mo = 1; y++ }
    guard++
  }
  return out
}

function fmtShares(qty: string): string {
  const n = parseFloat(qty)
  if (!Number.isFinite(n)) return qty
  return n.toLocaleString("en-GB", { minimumFractionDigits: 0, maximumFractionDigits: 4 })
}
