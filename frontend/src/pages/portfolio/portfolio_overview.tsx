import { useState } from "react"
import type React from "react"
import type {
  BreakdownItem,
  CashFlowMonth,
  Currency,
  Holding,
  InvestmentMetrics,
  PortfolioResponse,
} from "@/types"
import type { CategoryNode } from "@/bindings/CategoryNode"
import type { RemoteData } from "@/lib/remote_data"
import { visitRemoteData } from "@/lib/remote_data"
import type { PortfolioSummaryData } from "@/hooks/data"
import { useUrlFilters } from "@/hooks/use_url_filters"
import { PortfolioOverviewSkeleton } from "@/components/skeletons"
import { AuthAwareError } from "@/components/auth_aware_error"
import { ReloadingOverlay } from "@/components/reloading_overlay"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip"
import { Switch } from "@/components/ui/switch"
import { Badge } from "@/components/ui/badge"
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover"
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList } from "@/components/ui/command"
import { MoneyDisplay, DualAmount } from "@/components/currency"
import { InteractivePie } from "@/components/charts"
import type { PieDataItem } from "@/components/charts/interactive_pie"
import {
  TrendingUp, TrendingDown, Wallet, PiggyBank, Building2, Shield,
  ArrowUpRight, ArrowDownRight, BarChart3, Lock, CreditCard, Home,
  Landmark, Banknote, Settings2, Check, ChevronsUpDown, X,
} from "lucide-react"
import { ACCOUNT_TYPE_COLORS, ACCOUNT_TYPE_LABELS } from "@/lib/colors"
import { formatCurrency } from "@/lib/utils"
import { cn } from "@/lib/utils"
import type { AssetClass } from "@/bindings/AssetClass"
import { ASSET_CLASSES, type AssetClassSettings } from "@/hooks/use_url_filters"
import { accountTypeToAssetClass } from "@/lib/account_type_utils"


const ASSET_CLASS_COLORS: Record<AssetClass, string> = {
  Investments: "#a855f7",
  Pension:     "#6366f1",
  Cash:        "#22c55e",
  Property:    "#14b8a6",
}

const STOCK_COLORS = [
  "#3b82f6", "#f97316", "#22c55e", "#a855f7", "#ec4899",
  "#06b6d4", "#eab308", "#6366f1", "#14b8a6", "#ef4444",
]

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

function SettingsRow({ id, label, checked, onChange }: {
  id: string
  label: string
  checked: boolean
  onChange: (v: boolean) => void
}) {
  return (
    <div className="flex items-center justify-between gap-4">
      <label htmlFor={id} className="text-xs leading-tight cursor-pointer text-neutral-700 dark:text-neutral-200 whitespace-nowrap">
        {label}
      </label>
      <Switch id={id} size="sm" checked={checked} onCheckedChange={onChange} />
    </div>
  )
}

export function PortfolioOverview({
  data,
  dateLabel,
  assetClassSettings,
  hideSmall,
  categoryTree,
  excludedCategories,
  setExcludedCategories,
}: {
  data: RemoteData<PortfolioSummaryData>
  dateLabel?: string
  assetClassSettings: AssetClassSettings
  hideSmall: boolean
  categoryTree: CategoryNode[]
  excludedCategories: string[]
  setExcludedCategories: (ids: string[]) => void
}) {
  return visitRemoteData(data, {
    notLoaded: () => <PortfolioOverviewSkeleton />,
    failed: (error) => <AuthAwareError error={error} />,
    hasValue: ({ portfolio, history, cashFlow, allHoldings, currencies }) => {
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
            currencies={currencies}
            investmentMetrics={portfolio.investment_metrics}
            assetClassSettings={assetClassSettings}
            hideSmall={hideSmall}
            categoryTree={categoryTree}
            excludedCategories={excludedCategories}
            setExcludedCategories={setExcludedCategories}
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
  currencies?: Currency[]
  investmentMetrics?: InvestmentMetrics
  assetClassSettings: AssetClassSettings
  hideSmall: boolean
  categoryTree: CategoryNode[]
  excludedCategories: string[]
  setExcludedCategories: (ids: string[]) => void
}

function PortfolioOverviewInternal({
  portfolio,
  startNetWorth,
  endNetWorth,
  cashFlow = [],
  holdings = [],
  currencies = [] as Currency[],
  investmentMetrics,
  assetClassSettings,
  hideSmall,
  categoryTree,
  excludedCategories,
  setExcludedCategories,
}: PortfolioOverviewProps) {
  const { setFilter } = useUrlFilters()
  const [settingsOpen, setSettingsOpen] = useState(false)
  const [cashFlowSettingsOpen, setCashFlowSettingsOpen] = useState(false)


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

  const totalIncome = cashFlow.reduce((s, m) => s + parseFloat(m.income), 0)
  const totalSpending = cashFlow.reduce((s, m) => s + parseFloat(m.spending), 0)
  const monthCount = cashFlow.length || 1
  const avgIncome = totalIncome / monthCount
  const avgSpending = totalSpending / monthCount

  const fxRates = new Map<string, number>()
  for (const c of currencies) fxRates.set(c.code, parseFloat(c.fx_rate))
  const toPreferred = (value: number, currency: string) =>
    value * (fxRates.get(currency) ?? 1)

  // Build account → asset class lookup
  const accountAssetClass = new Map<string, AssetClass>(
    portfolio.accounts.map(a => [a.id, accountTypeToAssetClass(a.type)])
  )

  // Build full ungrouped data first to assign stable colors, then apply grouping
  const allPieItems = buildPieData({
    assetClassSettings,
    holdings,
    accountAssetClass,
    toPreferred,
    byAssetClass: portfolio.by_asset_class,
  })

  // Assign stable colors: merged classes use ASSET_CLASS_COLORS, split holdings cycle a palette
  const pieColorMap = new Map<string, string>()
  let splitColorIdx = 0
  allPieItems.forEach(d => {
    const isMergedClass = ASSET_CLASSES.includes(d.name as AssetClass)
    const color = isMergedClass
      ? (ASSET_CLASS_COLORS[d.name as AssetClass] ?? "#78716c")
      : STOCK_COLORS[splitColorIdx++ % STOCK_COLORS.length]
    pieColorMap.set(d.name, color)
  })
  pieColorMap.set("Others", "#78716c")

  const pieData = hideSmall ? groupSmall(allPieItems) : allPieItems
  const pieTotal = pieData.reduce((s, d) => s + d.value, 0)

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

      {/* Income/Outgoing + Portfolio pie */}
      <div className="grid gap-4 md:grid-cols-2">
        {/* Income, Spending & Investments card */}
        <Card className="overflow-hidden py-0 gap-0 min-h-[300px]">
          <div className="flex h-full">
            <div className="@container/cashflow flex-1 min-w-0 flex flex-col">
              <div className="flex items-center justify-between pt-4 pl-4 pr-4 pb-2 @[380px]/cashflow:pt-5 @[380px]/cashflow:pl-5 @[380px]/cashflow:pr-5">
                <p className="text-xs @[380px]/cashflow:text-sm font-medium text-muted-foreground flex items-center gap-1.5 @[380px]/cashflow:gap-2 min-w-0">
                  <BarChart3 className="h-3.5 w-3.5 @[380px]/cashflow:h-4 @[380px]/cashflow:w-4 shrink-0" />
                  <span className="truncate">Income, Spending & Investments</span>
                </p>
                {!cashFlowSettingsOpen && (
                  <button
                    onClick={() => setCashFlowSettingsOpen(true)}
                    className="rounded-md p-1 transition-colors hover:bg-muted relative shrink-0"
                    aria-label="Cash flow settings"
                  >
                    <Settings2 className="h-4 w-4" />
                    {excludedCategories.length > 0 && (
                      <span className="absolute -top-0.5 -right-0.5 h-2 w-2 rounded-full bg-orange-500" />
                    )}
                  </button>
                )}
              </div>
              <div className="px-4 pb-4 @[380px]/cashflow:px-5 @[380px]/cashflow:pb-5 flex-1 min-h-0">
                <div className="grid grid-cols-3 gap-2 @[380px]/cashflow:gap-4">
                  <div className="space-y-0.5 @[380px]/cashflow:space-y-1 min-w-0">
                    <div className="flex items-center gap-1 @[380px]/cashflow:gap-1.5 text-[10px] @[380px]/cashflow:text-xs text-muted-foreground whitespace-nowrap">
                      <ArrowUpRight className="h-3 w-3 text-green-500 shrink-0" />
                      <span className="truncate">Total Income</span>
                    </div>
                    <p className="text-sm @[300px]/cashflow:text-base @[380px]/cashflow:text-xl font-semibold text-green-500 tabular-nums truncate">
                      {formatCurrency(totalIncome.toFixed(2), preferredCurrency)}
                    </p>
                    <p className="text-[10px] @[380px]/cashflow:text-xs text-muted-foreground truncate">
                      ~{formatCurrency(avgIncome.toFixed(2), preferredCurrency)}/mo
                    </p>
                  </div>
                  <div className="space-y-0.5 @[380px]/cashflow:space-y-1 min-w-0">
                    <div className="flex items-center gap-1 @[380px]/cashflow:gap-1.5 text-[10px] @[380px]/cashflow:text-xs text-muted-foreground whitespace-nowrap">
                      <ArrowDownRight className="h-3 w-3 text-red-500 shrink-0" />
                      <span className="truncate">Total Spending</span>
                    </div>
                    <p className="text-sm @[300px]/cashflow:text-base @[380px]/cashflow:text-xl font-semibold text-red-500 tabular-nums truncate">
                      {formatCurrency(totalSpending.toFixed(2), preferredCurrency)}
                    </p>
                    <p className="text-[10px] @[380px]/cashflow:text-xs text-muted-foreground truncate">
                      ~{formatCurrency(avgSpending.toFixed(2), preferredCurrency)}/mo
                    </p>
                  </div>
                  <div className="space-y-0.5 @[380px]/cashflow:space-y-1 min-w-0">
                    <div className="text-[10px] @[380px]/cashflow:text-xs text-muted-foreground whitespace-nowrap truncate">
                      Net Savings
                    </div>
                    <p className={`text-sm @[300px]/cashflow:text-base @[380px]/cashflow:text-xl font-semibold tabular-nums truncate ${totalIncome - totalSpending >= 0 ? "text-green-500" : "text-red-500"}`}>
                      {formatCurrency((totalIncome - totalSpending).toFixed(2), preferredCurrency)}
                    </p>
                    <p className="text-[10px] @[380px]/cashflow:text-xs text-muted-foreground truncate">
                      ~{formatCurrency(((totalIncome - totalSpending) / monthCount).toFixed(2), preferredCurrency)}/mo
                    </p>
                  </div>
                </div>

                {investmentMetrics && (() => {
                  const startValue = parseFloat(investmentMetrics.start_value)
                  if (startValue <= 0) return null
                  const totalGrowth = parseFloat(investmentMetrics.total_growth)
                  const marketGrowth = parseFloat(investmentMetrics.market_growth)
                  const newCashInvested = parseFloat(investmentMetrics.new_cash_invested)
                  return (
                    <div className="mt-3 @[380px]/cashflow:mt-4 border-t pt-2 @[380px]/cashflow:pt-3">
                      <p className="text-[10px] @[380px]/cashflow:text-xs font-medium text-muted-foreground mb-1.5 @[380px]/cashflow:mb-2 uppercase tracking-wider">Investments</p>
                      <div className="grid grid-cols-3 gap-2 @[380px]/cashflow:gap-4">
                        <div className="space-y-0.5 min-w-0">
                          <p className="text-[10px] @[380px]/cashflow:text-xs text-muted-foreground whitespace-nowrap truncate">New Cash Invested</p>
                          <p className="text-sm @[380px]/cashflow:text-base font-semibold tabular-nums truncate">
                            {formatCurrency(newCashInvested.toFixed(2), preferredCurrency)}
                          </p>
                        </div>
                        <div className="space-y-0.5 min-w-0">
                          <p className="text-[10px] @[380px]/cashflow:text-xs text-muted-foreground whitespace-nowrap truncate">Total Growth</p>
                          <p className={`text-sm @[380px]/cashflow:text-base font-semibold tabular-nums truncate ${totalGrowth >= 0 ? "text-green-500" : "text-red-500"}`}>
                            {totalGrowth >= 0 ? "+" : ""}
                            {formatCurrency(totalGrowth.toFixed(2), preferredCurrency)}
                          </p>
                        </div>
                        <div className="space-y-0.5 min-w-0">
                          <p className="text-[10px] @[380px]/cashflow:text-xs text-muted-foreground whitespace-nowrap truncate">Market Performance</p>
                          <p className={`text-sm @[380px]/cashflow:text-base font-semibold tabular-nums truncate ${marketGrowth >= 0 ? "text-green-500" : "text-red-500"}`}>
                            {marketGrowth >= 0 ? "+" : ""}
                            {formatCurrency(marketGrowth.toFixed(2), preferredCurrency)}
                          </p>
                          <p className="text-[10px] @[380px]/cashflow:text-xs text-muted-foreground truncate">
                            {((marketGrowth / startValue) * 100).toFixed(1)}% return
                          </p>
                        </div>
                      </div>
                    </div>
                  )
                })()}
              </div>
            </div>

            {/* Settings panel — slides in from the right */}
            <div
              className={cn(
                "flex flex-col overflow-hidden transition-all duration-300 border-l bg-neutral-100 dark:bg-neutral-800",
                cashFlowSettingsOpen ? "w-72 opacity-100" : "w-0 opacity-0 border-transparent",
              )}
            >
              <div className="flex flex-col min-w-72 h-full overflow-hidden">
                <div className="flex items-center justify-between px-5 pt-5 pb-3 shrink-0">
                  <p className="text-xs font-semibold text-neutral-400 uppercase tracking-wider whitespace-nowrap">Cash Flow Settings</p>
                  <button
                    onClick={() => setCashFlowSettingsOpen(false)}
                    className="rounded-md p-1 transition-colors hover:bg-neutral-200 dark:hover:bg-neutral-700 text-neutral-500 dark:text-neutral-400"
                    aria-label="Close cash flow settings"
                  >
                    <Settings2 className="h-4 w-4" />
                  </button>
                </div>
                <div className="flex flex-col gap-3 px-5 pb-5 overflow-y-auto">
                  <div className="flex flex-col gap-2">
                    <label className="text-xs leading-tight text-neutral-700 dark:text-neutral-200">
                      Exclude categories
                    </label>
                    <p className="text-[11px] text-neutral-500 dark:text-neutral-400 leading-snug">
                      Removes matching transactions from Income, Spending &amp; Net Savings. Selecting a parent excludes all its children.
                    </p>
                    <CategoryExcludePicker
                      tree={categoryTree}
                      selected={excludedCategories}
                      onChange={setExcludedCategories}
                    />
                    {excludedCategories.length > 0 && (
                      <button
                        onClick={() => setExcludedCategories([])}
                        className="self-start text-[11px] text-neutral-500 dark:text-neutral-400 hover:text-neutral-700 dark:hover:text-neutral-200 underline underline-offset-2"
                      >
                        Clear all
                      </button>
                    )}
                  </div>
                </div>
              </div>
            </div>
          </div>
        </Card>

        {/* Portfolio breakdown pie */}
        <Card className="overflow-hidden py-0 gap-0 h-[300px]">
            <div className="flex h-full">
              {/* Main pie area */}
              <div className="flex-1 min-w-0 flex flex-col">
                <div className="flex items-center justify-between pt-5 pl-5 pr-5 pb-2">
                  <p className="text-sm font-medium text-muted-foreground">Portfolio Overview</p>
                  {/* Cog shown here only when settings panel is closed */}
                  {!settingsOpen && (
                    <button
                      onClick={() => setSettingsOpen(true)}
                      className="rounded-md p-1 transition-colors hover:bg-muted"
                      aria-label="Chart settings"
                    >
                      <Settings2 className="h-4 w-4" />
                    </button>
                  )}
                </div>
                <div className="px-5 pb-5 flex-1 min-h-0 flex">
                  {pieData.length > 0 ? (
                    <InteractivePie
                      data={pieData}
                      colorMap={pieColorMap}
                      height={240}
                      innerRadius={50}
                      outerRadius={90}
                      label={formatCurrency(pieTotal.toFixed(2), preferredCurrency)}
                      legendPosition="left"
                      className="w-full h-full"
                    />
                  ) : (
                    <div className="flex-1 flex items-center justify-center">
                      <p className="text-sm text-muted-foreground">No asset classes selected</p>
                    </div>
                  )}
                </div>
              </div>

              {/* Settings panel — slides in from right, full card height */}
              <div
                className={cn(
                  "flex flex-col overflow-hidden transition-all duration-300 border-l bg-neutral-100 dark:bg-neutral-800",
                  settingsOpen ? "w-64 opacity-100" : "w-0 opacity-0 border-transparent"
                )}
              >
                <div className="flex flex-col min-w-64 h-full overflow-hidden">
                  {/* Header — fixed, doesn't scroll */}
                  <div className="flex items-center justify-between px-5 pt-5 pb-3 shrink-0">
                    <p className="text-xs font-semibold text-neutral-400 uppercase tracking-wider whitespace-nowrap">Chart Settings</p>
                    <button
                      onClick={() => setSettingsOpen(false)}
                      className="rounded-md p-1 transition-colors hover:bg-neutral-200 dark:hover:bg-neutral-700 text-neutral-500 dark:text-neutral-400"
                      aria-label="Close chart settings"
                    >
                      <Settings2 className="h-4 w-4" />
                    </button>
                  </div>
                  {/* Scrollable rows */}
                  <div className="flex flex-col gap-3 px-5 pb-5 overflow-y-auto">
                    <SettingsRow
                      id="group-small"
                      label="Group <1% as Others"
                      checked={hideSmall}
                      onChange={v => setFilter({ hide_small: v ? undefined : "0" })}
                    />
                    {ASSET_CLASSES.map(cls => (
                      <div key={cls} className="flex flex-col gap-2">
                        <SettingsRow
                          id={`show-${cls.toLowerCase()}`}
                          label={`Show '${cls}'`}
                          checked={assetClassSettings[cls].show}
                          onChange={v => setFilter({ [`show_${cls.toLowerCase()}`]: v ? undefined : "0" })}
                        />
                        <SettingsRow
                          id={`merge-${cls.toLowerCase()}`}
                          label={`Merge '${cls}' holdings`}
                          checked={assetClassSettings[cls].merge}
                          onChange={v => setFilter({ [`merge_${cls.toLowerCase()}`]: v ? undefined : "0" })}
                        />
                      </div>
                    ))}
                  </div>
                </div>
              </div>
            </div>
          </Card>
      </div>

      {/* Breakdown cards */}
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

function CategoryExcludePicker({
  tree,
  selected,
  onChange,
}: {
  tree: CategoryNode[]
  selected: string[]
  onChange: (ids: string[]) => void
}) {
  const [open, setOpen] = useState(false)
  const selectedSet = new Set(selected)

  const idToLabel = new Map<string, string>()
  for (const parent of tree) {
    idToLabel.set(parent.id, parent.name)
    for (const child of parent.children) {
      idToLabel.set(child.id, `${parent.name}: ${child.name}`)
    }
  }

  function toggle(id: string) {
    const next = new Set(selectedSet)
    if (next.has(id)) next.delete(id)
    else next.add(id)
    onChange(Array.from(next))
  }

  return (
    <div className="flex flex-col gap-1.5">
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger className="inline-flex items-center justify-between gap-1 rounded-md border bg-background px-3 py-1.5 text-xs font-medium shadow-xs hover:bg-accent hover:text-accent-foreground h-8">
          <span className="text-neutral-500 dark:text-neutral-400">
            {selected.length === 0 ? "Select categories…" : `${selected.length} selected`}
          </span>
          <ChevronsUpDown className="h-3 w-3 opacity-50" />
        </PopoverTrigger>
        <PopoverContent className="w-[280px] p-0" align="start" sideOffset={6}>
          <Command>
            <CommandInput placeholder="Search categories…" />
            <CommandList>
              <CommandEmpty>
                {tree.length === 0 ? (
                  <span className="text-muted-foreground">Loading categories…</span>
                ) : (
                  "No results."
                )}
              </CommandEmpty>
              {tree.map(parent => (
                <CommandGroup key={parent.id} heading={parent.name}>
                  <CommandItem
                    value={`${parent.name} (all)`}
                    onSelect={() => toggle(parent.id)}
                  >
                    <Check className={cn("mr-2 h-4 w-4", selectedSet.has(parent.id) ? "opacity-100" : "opacity-0")} />
                    <span className="font-medium">All {parent.name}</span>
                  </CommandItem>
                  {parent.children.map(child => {
                    const isCheckedByParent = selectedSet.has(parent.id)
                    const isCheckedDirectly = selectedSet.has(child.id)
                    const isChecked = isCheckedByParent || isCheckedDirectly
                    return (
                      <CommandItem
                        key={child.id}
                        value={`${parent.name} ${child.name}`}
                        disabled={isCheckedByParent}
                        onSelect={() => { if (!isCheckedByParent) toggle(child.id) }}
                      >
                        <Check className={cn("mr-2 h-4 w-4", isChecked ? "opacity-100" : "opacity-0")} />
                        <span className={cn("pl-3", isCheckedByParent && "text-muted-foreground")}>{child.name}</span>
                      </CommandItem>
                    )
                  })}
                </CommandGroup>
              ))}
            </CommandList>
          </Command>
        </PopoverContent>
      </Popover>

      {selected.length > 0 && (
        <div className="flex flex-wrap gap-1">
          {selected.map(id => (
            <Badge key={id} variant="secondary" className="text-[11px] gap-1 pr-1">
              <span className="truncate max-w-[180px]">{idToLabel.get(id) ?? id}</span>
              <button
                onClick={() => toggle(id)}
                className="rounded-sm hover:bg-neutral-300 dark:hover:bg-neutral-600 p-0.5"
                aria-label={`Remove ${idToLabel.get(id) ?? id}`}
              >
                <X className="h-3 w-3" />
              </button>
            </Badge>
          ))}
        </div>
      )}
    </div>
  )
}

function groupSmall(items: PieDataItem[]): PieDataItem[] {
  const total = items.reduce((s, d) => s + d.value, 0)
  if (total === 0) return items
  const main = items.filter(d => (d.value / total) * 100 >= 1)
  const small = items.filter(d => (d.value / total) * 100 < 1)
  if (small.length === 0) return main
  const othersValue = parseFloat(small.reduce((s, d) => s + d.value, 0).toFixed(2))
  return [
    ...main,
    {
      name: "Others",
      value: othersValue,
      otherItems: small.map(d => ({ name: d.fullName ?? d.name, value: d.value })),
    },
  ]
}

function buildPieData({
  assetClassSettings,
  holdings,
  accountAssetClass,
  toPreferred,
  byAssetClass,
}: {
  assetClassSettings: AssetClassSettings
  holdings: Holding[]
  accountAssetClass: Map<string, AssetClass>
  toPreferred: (value: number, currency: string) => number
  byAssetClass: BreakdownItem[]
}): PieDataItem[] {
  const items: PieDataItem[] = []

  for (const cls of ASSET_CLASSES) {
    const { show, merge } = assetClassSettings[cls]
    if (!show) continue

    if (merge) {
      const item = byAssetClass.find(b => b.label === cls)
      if (item) {
        const value = parseFloat(parseFloat(item.value).toFixed(2))
        if (value > 0) items.push({ name: cls, value })
      }
    } else {
      const byName = new Map<string, { value: number; fullName: string }>()
      let negativesSum = 0
      for (const h of holdings.filter(h => accountAssetClass.get(h.account_id) === cls)) {
        const converted = toPreferred(parseFloat(h.value), h.currency)
        if (converted < 0) {
          negativesSum += converted
          continue
        }
        const key = h.short_name ?? h.symbol
        const existing = byName.get(key)
        if (existing) existing.value += converted
        else byName.set(key, { value: converted, fullName: h.name })
      }
      const positiveTotal = Array.from(byName.values()).reduce((s, e) => s + e.value, 0)
      const scale = positiveTotal > 0 && negativesSum < 0
        ? (positiveTotal + negativesSum) / positiveTotal
        : 1
      for (const [name, { value, fullName }] of byName) {
        const v = parseFloat((value * scale).toFixed(2))
        if (v > 0) items.push({ name, fullName, value: v })
      }
    }
  }

  return items.sort((a, b) => b.value - a.value)
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
