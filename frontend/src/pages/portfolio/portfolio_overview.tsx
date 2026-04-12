import type { PortfolioResponse, CashFlowMonth, Holding } from "@/types"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Currency } from "@/components/currency"
import { InteractivePie } from "@/components/charts"
import {
  TrendingUp, TrendingDown, Wallet, PiggyBank, Building2, Shield,
  ArrowUpRight, ArrowDownRight, BarChart3,
} from "lucide-react"
import { ACCOUNT_TYPE_COLORS, ACCOUNT_TYPE_LABELS } from "@/lib/colors"
import { formatCurrency } from "@/lib/utils"

interface InvestmentMetrics {
  totalGrowth: number
  newCashInvested: number
  marketGrowth: number
  startValue: number
  endValue: number
}

interface PortfolioOverviewProps {
  portfolio: PortfolioResponse
  startNetWorth?: string
  endNetWorth?: string
  dateLabel?: string
  cashFlow?: CashFlowMonth[]
  holdings?: Holding[]
  investmentMetrics?: InvestmentMetrics
}

export function PortfolioOverview({
  portfolio,
  startNetWorth,
  endNetWorth,
  cashFlow = [],
  holdings = [],
  investmentMetrics,
}: PortfolioOverviewProps) {
  const startNw = startNetWorth ? parseFloat(startNetWorth) : null
  const endNw = endNetWorth ? parseFloat(endNetWorth) : null
  const delta = startNw !== null && endNw !== null ? endNw - startNw : null
  const deltaPercent =
    delta !== null && startNw !== null && startNw > 0
      ? ((delta / startNw) * 100).toFixed(1)
      : null

  const netWorth = parseFloat(portfolio.net_worth)
  const available = parseFloat(portfolio.available_wealth)
  const availablePct = netWorth > 0 ? (available / netWorth) * 100 : 0

  // Income/outgoing totals
  const totalIncome = cashFlow.reduce((s, m) => s + parseFloat(m.income), 0)
  const totalSpending = cashFlow.reduce((s, m) => s + parseFloat(m.spending), 0)
  const monthCount = cashFlow.length || 1
  const avgIncome = totalIncome / monthCount
  const avgSpending = totalSpending / monthCount

  // Stocks breakdown (aggregate holdings by short_name)
  const holdingsByName = new Map<string, { value: number; fullName: string }>()
  for (const h of holdings) {
    const key = h.short_name
    const existing = holdingsByName.get(key)
    if (existing) {
      existing.value += parseFloat(h.value)
    } else {
      holdingsByName.set(key, { value: parseFloat(h.value), fullName: h.name })
    }
  }
  const stocksData = Array.from(holdingsByName.entries())
    .map(([shortName, { value }]) => ({
      name: shortName,
      value: parseFloat(value.toFixed(2)),
    }))
    .sort((a, b) => b.value - a.value)

  const STOCK_COLORS = [
    "#3b82f6", "#f97316", "#22c55e", "#a855f7", "#ec4899",
    "#06b6d4", "#eab308", "#6366f1", "#14b8a6", "#ef4444",
  ]

  return (
    <div className="space-y-4">
      {/* Top row: Net worth + Balance sheet */}
      <div className="grid gap-4 md:grid-cols-3">
        <Card className="md:col-span-2">
          <CardHeader className="pb-2">
            <CardTitle className="text-sm font-medium text-muted-foreground flex items-center gap-2">
              <Wallet className="h-4 w-4" />
              Net Worth
            </CardTitle>
          </CardHeader>
          <CardContent>
            <div className="flex items-baseline gap-3">
              <span className="text-4xl font-bold tabular-nums">
                <Currency amount={portfolio.net_worth} colorize={false} />
              </span>
              {delta !== null && (
                <div className="flex flex-col">
                  <span
                    className={`flex items-center gap-1 text-sm font-semibold ${
                      delta >= 0 ? "text-green-500" : "text-red-500"
                    }`}
                  >
                    {delta >= 0 ? <TrendingUp className="h-4 w-4" /> : <TrendingDown className="h-4 w-4" />}
                    <Currency amount={delta.toFixed(2)} />
                    {deltaPercent && <span className="text-xs opacity-75">({deltaPercent}%)</span>}
                  </span>
                  <span className="text-xs text-muted-foreground ml-5">over selected period</span>
                </div>
              )}
            </div>
            <div className="mt-4 space-y-2">
              <div className="flex justify-between text-sm">
                <span className="flex items-center gap-1.5">
                  <PiggyBank className="h-3.5 w-3.5 text-blue-500" />
                  Available
                  <span className="font-medium"><Currency amount={portfolio.available_wealth} colorize={false} /></span>
                </span>
                <span className="flex items-center gap-1.5">
                  <Shield className="h-3.5 w-3.5 text-orange-500" />
                  Unavailable
                  <span className="font-medium"><Currency amount={portfolio.unavailable_wealth} colorize={false} /></span>
                </span>
              </div>
              <div className="h-3 rounded-full bg-orange-500/20 overflow-hidden">
                <div className="h-full rounded-full bg-blue-500 transition-all duration-500" style={{ width: `${availablePct}%` }} />
              </div>
              <div className="flex justify-between text-xs text-muted-foreground">
                <span>{availablePct.toFixed(0)}% liquid</span>
                <span>{(100 - availablePct).toFixed(0)}% locked</span>
              </div>
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm font-medium text-muted-foreground flex items-center gap-2">
              <Building2 className="h-4 w-4" />
              Balance Sheet
            </CardTitle>
          </CardHeader>
          <CardContent className="space-y-3">
            <div className="flex justify-between items-center">
              <span className="text-sm text-muted-foreground">Assets</span>
              <span className="text-lg font-semibold text-green-500 tabular-nums">
                <Currency amount={portfolio.total_assets} colorize={false} />
              </span>
            </div>
            <div className="flex justify-between items-center">
              <span className="text-sm text-muted-foreground">Liabilities</span>
              <span className="text-lg font-semibold text-red-500 tabular-nums">
                <Currency amount={portfolio.total_liabilities} colorize={false} />
              </span>
            </div>
            <div className="border-t pt-2 flex justify-between items-center">
              <span className="text-sm font-medium">Net</span>
              <span className="text-lg font-bold tabular-nums">
                <Currency amount={portfolio.net_worth} colorize={false} />
              </span>
            </div>
          </CardContent>
        </Card>
      </div>

      {/* Income/Outgoing + Stocks breakdown */}
      <div className="grid gap-4 md:grid-cols-2">
        {/* Income, Spending & Investments card */}
        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm font-medium text-muted-foreground flex items-center gap-2">
              <BarChart3 className="h-4 w-4" />
              Income, Spending & Investments
            </CardTitle>
          </CardHeader>
          <CardContent>
            <div className="grid grid-cols-3 gap-4">
              <div className="space-y-1">
                <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
                  <ArrowUpRight className="h-3 w-3 text-green-500" />
                  Total Income
                </div>
                <p className="text-xl font-semibold text-green-500 tabular-nums">
                  {formatCurrency(totalIncome.toFixed(2))}
                </p>
                <p className="text-xs text-muted-foreground">
                  ~{formatCurrency(avgIncome.toFixed(2))}/mo
                </p>
              </div>
              <div className="space-y-1">
                <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
                  <ArrowDownRight className="h-3 w-3 text-red-500" />
                  Total Spending
                </div>
                <p className="text-xl font-semibold text-red-500 tabular-nums">
                  {formatCurrency(totalSpending.toFixed(2))}
                </p>
                <p className="text-xs text-muted-foreground">
                  ~{formatCurrency(avgSpending.toFixed(2))}/mo
                </p>
              </div>
              <div className="space-y-1">
                <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
                  Net Savings
                </div>
                <p className={`text-xl font-semibold tabular-nums ${totalIncome - totalSpending >= 0 ? "text-green-500" : "text-red-500"}`}>
                  {formatCurrency((totalIncome - totalSpending).toFixed(2))}
                </p>
                <p className="text-xs text-muted-foreground">
                  ~{formatCurrency(((totalIncome - totalSpending) / monthCount).toFixed(2))}/mo
                </p>
              </div>
            </div>

            {/* Investment metrics */}
            {investmentMetrics && investmentMetrics.startValue > 0 && (
              <div className="mt-4 border-t pt-3">
                <p className="text-xs font-medium text-muted-foreground mb-2 uppercase tracking-wider">Investments</p>
                <div className="grid grid-cols-3 gap-4">
                  <div className="space-y-0.5">
                    <p className="text-xs text-muted-foreground">New Cash Invested</p>
                    <p className="text-base font-semibold tabular-nums">
                      {formatCurrency(investmentMetrics.newCashInvested.toFixed(2))}
                    </p>
                  </div>
                  <div className="space-y-0.5">
                    <p className="text-xs text-muted-foreground">Total Growth</p>
                    <p className={`text-base font-semibold tabular-nums ${investmentMetrics.totalGrowth >= 0 ? "text-green-500" : "text-red-500"}`}>
                      {investmentMetrics.totalGrowth >= 0 ? "+" : ""}
                      {formatCurrency(investmentMetrics.totalGrowth.toFixed(2))}
                    </p>
                  </div>
                  <div className="space-y-0.5">
                    <p className="text-xs text-muted-foreground">Market Performance</p>
                    <p className={`text-base font-semibold tabular-nums ${investmentMetrics.marketGrowth >= 0 ? "text-green-500" : "text-red-500"}`}>
                      {investmentMetrics.marketGrowth >= 0 ? "+" : ""}
                      {formatCurrency(investmentMetrics.marketGrowth.toFixed(2))}
                    </p>
                    {investmentMetrics.startValue > 0 && (
                      <p className="text-xs text-muted-foreground">
                        {((investmentMetrics.marketGrowth / investmentMetrics.startValue) * 100).toFixed(1)}% return
                      </p>
                    )}
                  </div>
                </div>
              </div>
            )}
          </CardContent>
        </Card>

        {/* Stocks breakdown card */}
        {stocksData.length > 0 && (
          <Card>
            <CardHeader className="pb-2">
              <CardTitle className="text-sm font-medium text-muted-foreground">
                Holdings by Stock
              </CardTitle>
            </CardHeader>
            <CardContent>
              <InteractivePie
                data={stocksData}
                colors={STOCK_COLORS}
                height={220}
                innerRadius={50}
                outerRadius={85}
                label={formatCurrency(stocksData.reduce((s, d) => s + d.value, 0).toFixed(2))}
              />
            </CardContent>
          </Card>
        )}
      </div>

      {/* Breakdown cards */}
      <div className="grid gap-4 md:grid-cols-3">
        <BreakdownCard
          title="By Asset Type"
          items={portfolio.by_type}
          colorFn={(label) =>
            ACCOUNT_TYPE_COLORS[label as keyof typeof ACCOUNT_TYPE_COLORS] ?? "#78716c"
          }
          labelFn={(label) =>
            ACCOUNT_TYPE_LABELS[label as keyof typeof ACCOUNT_TYPE_LABELS] ?? label
          }
        />
        <BreakdownCard title="By Institution" items={portfolio.by_institution} />
        <BreakdownCard title="By Sector" items={portfolio.by_sector} />
      </div>
    </div>
  )
}

const BREAKDOWN_COLORS = [
  "#3b82f6", "#f97316", "#22c55e", "#a855f7", "#ec4899",
  "#06b6d4", "#eab308", "#6366f1",
]

function BreakdownCard({
  title,
  items,
  colorFn,
  labelFn,
}: {
  title: string
  items: { label: string; total: string; percent: number }[]
  colorFn?: (label: string) => string
  labelFn?: (label: string) => string
}) {
  return (
    <Card>
      <CardHeader className="pb-2">
        <CardTitle className="text-sm font-medium">{title}</CardTitle>
      </CardHeader>
      <CardContent className="space-y-3">
        {items.map((item, i) => {
          const color = colorFn
            ? colorFn(item.label)
            : BREAKDOWN_COLORS[i % BREAKDOWN_COLORS.length]
          const displayLabel = labelFn ? labelFn(item.label) : item.label
          return (
            <div key={item.label} className="space-y-1">
              <div className="flex items-center justify-between text-sm">
                <span className="flex items-center gap-1.5">
                  <span
                    className="inline-block h-2.5 w-2.5 rounded-sm shrink-0"
                    style={{ backgroundColor: color }}
                  />
                  <span className="capitalize">{displayLabel}</span>
                </span>
                <div className="flex items-center gap-2 tabular-nums">
                  <Currency amount={item.total} colorize={false} />
                  <span className="text-xs text-muted-foreground w-8 text-right">
                    {item.percent}%
                  </span>
                </div>
              </div>
              <div className="h-1.5 w-full rounded-full bg-muted overflow-hidden">
                <div
                  className="h-full rounded-full transition-all duration-500"
                  style={{
                    width: `${Math.abs(item.percent)}%`,
                    backgroundColor: color,
                  }}
                />
              </div>
            </div>
          )
        })}
      </CardContent>
    </Card>
  )
}
