import type { PortfolioResponse } from "@/types"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Currency } from "@/components/currency"
import { TrendingUp, TrendingDown } from "lucide-react"

interface PortfolioOverviewProps {
  portfolio: PortfolioResponse
  previousNetWorth?: string
}

export function PortfolioOverview({
  portfolio,
  previousNetWorth,
}: PortfolioOverviewProps) {
  const netWorth = parseFloat(portfolio.net_worth)
  const prevNw = previousNetWorth ? parseFloat(previousNetWorth) : null
  const delta = prevNw !== null ? netWorth - prevNw : null

  return (
    <div className="space-y-4">
      {/* Net worth card */}
      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="text-sm font-medium text-muted-foreground">
            Net Worth
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="flex items-baseline gap-3">
            <span className="text-3xl font-bold tabular-nums">
              <Currency
                amount={portfolio.net_worth}
                colorize={false}
              />
            </span>
            {delta !== null && (
              <span
                className={`flex items-center gap-1 text-sm font-medium ${
                  delta >= 0 ? "text-green-500" : "text-red-500"
                }`}
              >
                {delta >= 0 ? (
                  <TrendingUp className="h-4 w-4" />
                ) : (
                  <TrendingDown className="h-4 w-4" />
                )}
                <Currency amount={delta.toFixed(2)} />
              </span>
            )}
          </div>
          <div className="mt-2 flex gap-6 text-sm text-muted-foreground">
            <span>
              Available:{" "}
              <Currency
                amount={portfolio.available_wealth}
                colorize={false}
                className="font-medium text-foreground"
              />
            </span>
            <span>
              Unavailable:{" "}
              <Currency
                amount={portfolio.unavailable_wealth}
                colorize={false}
                className="font-medium text-foreground"
              />
            </span>
          </div>
          <div className="mt-1 flex gap-6 text-sm text-muted-foreground">
            <span>
              Assets:{" "}
              <Currency amount={portfolio.total_assets} colorize={false} />
            </span>
            <span>
              Liabilities:{" "}
              <Currency amount={portfolio.total_liabilities} colorize={false} />
            </span>
          </div>
        </CardContent>
      </Card>

      {/* Breakdown tables */}
      <div className="grid gap-4 md:grid-cols-3">
        <BreakdownCard title="By Asset Type" items={portfolio.by_type} />
        <BreakdownCard title="By Institution" items={portfolio.by_institution} />
        <BreakdownCard title="By Sector" items={portfolio.by_sector} />
      </div>
    </div>
  )
}

function BreakdownCard({
  title,
  items,
}: {
  title: string
  items: { label: string; total: string; percent: number }[]
}) {
  return (
    <Card>
      <CardHeader className="pb-2">
        <CardTitle className="text-sm font-medium">{title}</CardTitle>
      </CardHeader>
      <CardContent>
        <div className="space-y-2">
          {items.map((item) => (
            <div key={item.label} className="flex items-center justify-between text-sm">
              <span className="capitalize">{item.label}</span>
              <div className="flex items-center gap-2">
                <Currency amount={item.total} colorize={false} />
                <span className="w-10 text-right text-muted-foreground">
                  {item.percent}%
                </span>
              </div>
            </div>
          ))}
        </div>
      </CardContent>
    </Card>
  )
}
