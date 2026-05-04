import type React from "react"
import type {
  BreakdownItem,
  CashFlowMonth,
  Currency,
  Holding,
  InvestmentMetrics,
  PortfolioResponse,
} from "@/types"
import type { RemoteData } from "@/lib/remote_data"
import { visitRemoteData } from "@/lib/remote_data"
import type { PortfolioSummaryData } from "@/hooks/data"
import { PortfolioOverviewSkeleton } from "@/components/skeletons"
import { AuthAwareError } from "@/components/auth_aware_error"
import { ReloadingOverlay } from "@/components/reloading_overlay"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip"
import { MoneyDisplay, DualAmount } from "@/components/currency"
import { InteractivePie } from "@/components/charts"
import {
  TrendingUp, TrendingDown, Wallet, PiggyBank, Building2, Shield,
  ArrowUpRight, ArrowDownRight, BarChart3, Lock, CreditCard, Home,
  Landmark, Banknote,
} from "lucide-react"
import { ACCOUNT_TYPE_COLORS, ACCOUNT_TYPE_LABELS } from "@/lib/colors"
import { formatCurrency } from "@/lib/utils"

function RichTooltipContent({
  side, align, children,
}: {
  side?: "top" | "bottom" | "left" | "right"
  align?: "start" | "center" | "end"
  children: React.ReactNode
}) {
  return (
    <TooltipContent side={side} align={align} className="max-w-72 !p-0 overflow-hidden !bg-[#1c1c1c]" arrowClassName="!bg-[#1c1c1c] !fill-[#1c1c1c]">
      <div className="space-y-2 p-3" style={{ background: "#1c1c1c", color: "#f0f0f0" }}>
        {children}
      </div>
    </TooltipContent>
  )
}

export function PortfolioOverview({ data, dateLabel }: { data: RemoteData<PortfolioSummaryData>; dateLabel?: string }) {
  return visitRemoteData(data, {
    notLoaded: () => <PortfolioOverviewSkeleton />,
    failed: (error) => <AuthAwareError error={error} />,
    hasValue: ({ portfolio, history, cashFlow, allHoldings, currencies }) => {
      const pensionAccountIds = new Set(
        portfolio.accounts.filter(a => a.type === "pension").map(a => a.id)
      )
      const startNetWorth = history.length >= 1 ? history[0].total_wealth : undefined
      const endNetWorth = history.length >= 1 ? history[history.length - 1].total_wealth : undefined
      return (
        <div className="relative">
          <PortfolioOverviewInternal
            portfolio={portfolio}
            startNetWorth={startNetWorth}
            endNetWorth={endNetWorth}
            dateLabel={dateLabel}
            cashFlow={cashFlow}
            holdings={allHoldings}
            pensionAccountIds={pensionAccountIds}
            currencies={currencies}
            investmentMetrics={portfolio.investment_metrics}
          />
          <ReloadingOverlay active={data.status === "reloading"} />
        </div>
      )
    },
  })
}

interface PortfolioOverviewProps {
  portfolio: PortfolioResponse
  startNetWorth?: string
  endNetWorth?: string
  dateLabel?: string
  cashFlow?: CashFlowMonth[]
  holdings?: Holding[]
  pensionAccountIds?: Set<string>
  currencies?: Currency[]
  investmentMetrics?: InvestmentMetrics
}

function PortfolioOverviewInternal({
  portfolio,
  startNetWorth,
  endNetWorth,
  cashFlow = [],
  holdings = [],
  pensionAccountIds = new Set(),
  currencies = [] as Currency[],
  investmentMetrics,
}: PortfolioOverviewProps) {
  const preferredCurrency = portfolio.preferred_currency
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

  // Build FX rate lookup: currency code → rate-to-preferred (GBP = 1)
  const fxRates = new Map<string, number>()
  for (const c of currencies) {
    fxRates.set(c.code, parseFloat(c.fx_rate))
  }
  const toPreferred = (value: number, currency: string) =>
    value * (fxRates.get(currency) ?? 1)

  // Stocks breakdown — exclude pension holdings (tracked separately, skew the chart)
  const holdingsByName = new Map<string, { value: number; fullName: string }>()
  for (const h of holdings.filter(h => !pensionAccountIds.has(h.account_id))) {
    const key = h.short_name ?? h.symbol
    const converted = toPreferred(parseFloat(h.value), h.currency)
    const existing = holdingsByName.get(key)
    if (existing) {
      existing.value += converted
    } else {
      holdingsByName.set(key, { value: converted, fullName: h.name })
    }
  }
  const stocksData = Array.from(holdingsByName.entries())
    .map(([shortName, { value, fullName }]) => ({
      name: shortName,
      fullName,
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
                <MoneyDisplay amount={portfolio.net_worth} currency={preferredCurrency} colorize={false} />
              </span>
              {delta !== null && (
                <div className="flex flex-col">
                  <span
                    className={`flex items-center gap-1 text-sm font-semibold ${
                      delta >= 0 ? "text-green-500" : "text-red-500"
                    }`}
                  >
                    {delta >= 0 ? <TrendingUp className="h-4 w-4" /> : <TrendingDown className="h-4 w-4" />}
                    <MoneyDisplay amount={delta.toFixed(2)} currency={preferredCurrency} />
                    {deltaPercent && <span className="text-xs opacity-75">({deltaPercent}%)</span>}
                  </span>
                  <span className="text-xs text-muted-foreground ml-5">over selected period</span>
                </div>
              )}
            </div>
            <div className="mt-4 space-y-2">
              <TooltipProvider>
              <div className="flex justify-between text-sm">
                <span className="flex items-center gap-1.5">
                  <PiggyBank className="h-3.5 w-3.5 text-blue-500" />
                  <Tooltip>
                    <TooltipTrigger className="underline decoration-dotted underline-offset-2 cursor-default">
                      Available
                    </TooltipTrigger>
                    <RichTooltipContent side="top">
                      <div className="flex items-center gap-2">
                        <PiggyBank className="h-4 w-4 text-blue-400 shrink-0" />
                        <span className="font-semibold text-sm text-white">Liquid wealth</span>
                      </div>
                      <p className="text-xs text-white/70 leading-relaxed">Funds you can access directly or convert quickly.</p>
                      <ul className="space-y-1 text-xs text-white/60">
                        <li className="flex items-center gap-1.5"><Banknote className="h-3 w-3 shrink-0" /> Checking &amp; savings accounts</li>
                        <li className="flex items-center gap-1.5"><TrendingUp className="h-3 w-3 shrink-0" /> Investment portfolios</li>
                        <li className="flex items-center gap-1.5"><Landmark className="h-3 w-3 shrink-0" /> Cash &amp; money market</li>
                        <li className="flex items-center gap-1.5"><CreditCard className="h-3 w-3 shrink-0 text-red-400" /> Credit balances reduce this</li>
                      </ul>
                    </RichTooltipContent>
                  </Tooltip>
                  <span className="font-medium"><MoneyDisplay amount={portfolio.available_wealth} currency={preferredCurrency} colorize={false} /></span>
                </span>
                <span className="flex items-center gap-1.5">
                  <Shield className="h-3.5 w-3.5 text-orange-500" />
                  <Tooltip>
                    <TooltipTrigger className="underline decoration-dotted underline-offset-2 cursor-default">
                      Unavailable
                    </TooltipTrigger>
                    <RichTooltipContent side="top" align="end">
                      <div className="flex items-center gap-2">
                        <Lock className="h-4 w-4 text-orange-400 shrink-0" />
                        <span className="font-semibold text-sm text-white">Locked wealth</span>
                      </div>
                      <p className="text-xs text-white/70 leading-relaxed">Wealth tied up in illiquid or long-term assets.</p>
                      <ul className="space-y-1 text-xs text-white/60">
                        <li className="flex items-center gap-1.5"><Home className="h-3 w-3 shrink-0" /> Property equity</li>
                        <li className="flex items-center gap-1.5"><Shield className="h-3 w-3 shrink-0" /> Pension &amp; retirement pots</li>
                        <li className="flex items-center gap-1.5"><Landmark className="h-3 w-3 shrink-0 text-red-400" /> Mortgage &amp; secured debt</li>
                      </ul>
                    </RichTooltipContent>
                  </Tooltip>
                  <span className="font-medium"><MoneyDisplay amount={portfolio.unavailable_wealth} currency={preferredCurrency} colorize={false} /></span>
                </span>
              </div>
              </TooltipProvider>
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
                <MoneyDisplay amount={portfolio.total_assets} currency={preferredCurrency} colorize={false} />
              </span>
            </div>
            <div className="flex justify-between items-center">
              <span className="text-sm text-muted-foreground">Liabilities</span>
              <span className="text-lg font-semibold text-red-500 tabular-nums">
                <MoneyDisplay amount={portfolio.total_liabilities} currency={preferredCurrency} colorize={false} />
              </span>
            </div>
            <div className="border-t pt-2 flex justify-between items-center">
              <span className="text-sm font-medium">Net</span>
              <span className="text-lg font-bold tabular-nums">
                <MoneyDisplay amount={portfolio.net_worth} currency={preferredCurrency} colorize={false} />
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
            <div className="grid grid-cols-1 sm:grid-cols-3 gap-4">
              <div className="space-y-1">
                <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
                  <ArrowUpRight className="h-3 w-3 text-green-500" />
                  Total Income
                </div>
                <p className="text-xl font-semibold text-green-500 tabular-nums">
                  {formatCurrency(totalIncome.toFixed(2), preferredCurrency)}
                </p>
                <p className="text-xs text-muted-foreground">
                  ~{formatCurrency(avgIncome.toFixed(2), preferredCurrency)}/mo
                </p>
              </div>
              <div className="space-y-1">
                <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
                  <ArrowDownRight className="h-3 w-3 text-red-500" />
                  Total Spending
                </div>
                <p className="text-xl font-semibold text-red-500 tabular-nums">
                  {formatCurrency(totalSpending.toFixed(2), preferredCurrency)}
                </p>
                <p className="text-xs text-muted-foreground">
                  ~{formatCurrency(avgSpending.toFixed(2), preferredCurrency)}/mo
                </p>
              </div>
              <div className="space-y-1">
                <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
                  Net Savings
                </div>
                <p className={`text-xl font-semibold tabular-nums ${totalIncome - totalSpending >= 0 ? "text-green-500" : "text-red-500"}`}>
                  {formatCurrency((totalIncome - totalSpending).toFixed(2), preferredCurrency)}
                </p>
                <p className="text-xs text-muted-foreground">
                  ~{formatCurrency(((totalIncome - totalSpending) / monthCount).toFixed(2), preferredCurrency)}/mo
                </p>
              </div>
            </div>

            {/* Investment metrics - parsed from decimal strings in the API response */}
            {investmentMetrics && (() => {
              const startValue = parseFloat(investmentMetrics.start_value)
              if (startValue <= 0) return null
              const totalGrowth = parseFloat(investmentMetrics.total_growth)
              const marketGrowth = parseFloat(investmentMetrics.market_growth)
              const newCashInvested = parseFloat(investmentMetrics.new_cash_invested)
              return (
                <div className="mt-4 border-t pt-3">
                  <p className="text-xs font-medium text-muted-foreground mb-2 uppercase tracking-wider">Investments</p>
                  <div className="grid grid-cols-1 sm:grid-cols-3 gap-4">
                    <div className="space-y-0.5">
                      <p className="text-xs text-muted-foreground">New Cash Invested</p>
                      <p className="text-base font-semibold tabular-nums">
                        {formatCurrency(newCashInvested.toFixed(2), preferredCurrency)}
                      </p>
                    </div>
                    <div className="space-y-0.5">
                      <p className="text-xs text-muted-foreground">Total Growth</p>
                      <p className={`text-base font-semibold tabular-nums ${totalGrowth >= 0 ? "text-green-500" : "text-red-500"}`}>
                        {totalGrowth >= 0 ? "+" : ""}
                        {formatCurrency(totalGrowth.toFixed(2), preferredCurrency)}
                      </p>
                    </div>
                    <div className="space-y-0.5">
                      <p className="text-xs text-muted-foreground">Market Performance</p>
                      <p className={`text-base font-semibold tabular-nums ${marketGrowth >= 0 ? "text-green-500" : "text-red-500"}`}>
                        {marketGrowth >= 0 ? "+" : ""}
                        {formatCurrency(marketGrowth.toFixed(2), preferredCurrency)}
                      </p>
                      <p className="text-xs text-muted-foreground">
                        {((marketGrowth / startValue) * 100).toFixed(1)}% return
                      </p>
                    </div>
                  </div>
                </div>
              )
            })()}
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
                label={formatCurrency(stocksData.reduce((s, d) => s + d.value, 0).toFixed(2), preferredCurrency)}
              />
            </CardContent>
          </Card>
        )}
      </div>

      {/* Breakdown cards - each card hides itself when its underlying
          breakdown array is empty so the page collapses cleanly for
          accounts with limited data. The wrapping row also drops out
          entirely when all three are empty. */}
      {(portfolio.by_type.length > 0 ||
        portfolio.by_institution.length > 0 ||
        portfolio.by_asset_class.length > 0) && (
        <div className="grid gap-4 md:grid-cols-3">
          {portfolio.by_type.length > 0 && (
            <BreakdownCard
              title="By Asset Type"
              items={portfolio.by_type}
              preferredCurrency={preferredCurrency}
              colorFn={(label) =>
                ACCOUNT_TYPE_COLORS[label as keyof typeof ACCOUNT_TYPE_COLORS] ?? "#78716c"
              }
              labelFn={(label) =>
                ACCOUNT_TYPE_LABELS[label as keyof typeof ACCOUNT_TYPE_LABELS] ?? label
              }
            />
          )}
          {portfolio.by_institution.length > 0 && (
            <BreakdownCard title="By Institution" items={portfolio.by_institution} preferredCurrency={preferredCurrency} />
          )}
          {portfolio.by_asset_class.length > 0 && (
            <BreakdownCard title="By Asset Class" items={portfolio.by_asset_class} preferredCurrency={preferredCurrency} />
          )}
        </div>
      )}
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
  preferredCurrency,
  colorFn,
  labelFn,
}: {
  title: string
  items: BreakdownItem[]
  preferredCurrency: string
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
                <div className="flex items-center gap-2">
                  <DualAmount value={item.value} preferredCurrency={preferredCurrency} display={item.display_currency} secondaryFirst />
                  <span className={`text-xs w-10 text-right ${item.percentage < 0 ? "text-red-500" : "text-muted-foreground"}`}>
                    {item.percentage.toFixed(1)}%
                  </span>
                </div>
              </div>
              <div className="h-1.5 w-full rounded-full bg-muted overflow-hidden flex">
                <div
                  className="h-full rounded-full transition-all duration-500"
                  style={{
                    width: `${Math.abs(item.percentage)}%`,
                    backgroundColor: item.percentage < 0 ? "#ef4444" : color,
                    marginLeft: item.percentage < 0 ? "auto" : undefined,
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
