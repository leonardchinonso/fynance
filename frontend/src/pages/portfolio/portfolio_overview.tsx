import type { PortfolioResponse } from "@/types"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Currency } from "@/components/currency"
// Using plain divs for colored progress bars instead of Progress component
import { TrendingUp, TrendingDown, Wallet, PiggyBank, Building2, Shield } from "lucide-react"
import { ACCOUNT_TYPE_COLORS, ACCOUNT_TYPE_LABELS } from "@/lib/colors"

interface PortfolioOverviewProps {
  portfolio: PortfolioResponse
  startNetWorth?: string
  endNetWorth?: string
  dateLabel?: string
}

export function PortfolioOverview({
  portfolio,
  startNetWorth,
  endNetWorth,
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
  const unavailable = parseFloat(portfolio.unavailable_wealth)
  const availablePct = netWorth > 0 ? (available / netWorth) * 100 : 0

  return (
    <div className="space-y-4">
      {/* Top row: Net worth + Available/Unavailable breakdown */}
      <div className="grid gap-4 md:grid-cols-3">
        {/* Net Worth */}
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
                    {delta >= 0 ? (
                      <TrendingUp className="h-4 w-4" />
                    ) : (
                      <TrendingDown className="h-4 w-4" />
                    )}
                    <Currency amount={delta.toFixed(2)} />
                    {deltaPercent && (
                      <span className="text-xs opacity-75">
                        ({deltaPercent}%)
                      </span>
                    )}
                  </span>
                  <span className="text-xs text-muted-foreground ml-5">
                    over selected period
                  </span>
                </div>
              )}
            </div>

            {/* Available vs Unavailable bar */}
            <div className="mt-4 space-y-2">
              <div className="flex justify-between text-sm">
                <span className="flex items-center gap-1.5">
                  <PiggyBank className="h-3.5 w-3.5 text-blue-500" />
                  Available
                  <span className="font-medium">
                    <Currency amount={portfolio.available_wealth} colorize={false} />
                  </span>
                </span>
                <span className="flex items-center gap-1.5">
                  <Shield className="h-3.5 w-3.5 text-orange-500" />
                  Unavailable
                  <span className="font-medium">
                    <Currency amount={portfolio.unavailable_wealth} colorize={false} />
                  </span>
                </span>
              </div>
              <div className="h-3 rounded-full bg-orange-500/20 overflow-hidden">
                <div
                  className="h-full rounded-full bg-blue-500 transition-all duration-500"
                  style={{ width: `${availablePct}%` }}
                />
              </div>
              <div className="flex justify-between text-xs text-muted-foreground">
                <span>{availablePct.toFixed(0)}% liquid</span>
                <span>{(100 - availablePct).toFixed(0)}% locked</span>
              </div>
            </div>
          </CardContent>
        </Card>

        {/* Assets / Liabilities summary */}
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

      {/* Breakdown cards with visual bars */}
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
        <BreakdownCard
          title="By Institution"
          items={portfolio.by_institution}
        />
        <BreakdownCard
          title="By Sector"
          items={portfolio.by_sector}
        />
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
