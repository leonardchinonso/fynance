import type { PortfolioResponse, Holding } from "@/types"
import { InteractivePie } from "@/components/charts"
import { ACCOUNT_TYPE_COLORS } from "@/lib/colors"
import { EmptyState } from "@/components/empty_state"
import { formatCurrency } from "@/lib/utils"

const INST_COLORS = ["#3b82f6", "#f97316", "#22c55e", "#ec4899", "#06b6d4", "#eab308", "#6366f1"]
const ASSET_CLASS_COLORS = ["#3b82f6", "#f97316", "#22c55e", "#ec4899", "#a855f7"]
const STOCK_COLORS = [
  "#3b82f6", "#f97316", "#22c55e", "#a855f7", "#ec4899",
  "#06b6d4", "#eab308", "#6366f1", "#14b8a6", "#ef4444",
]

interface PortfolioChartsProps {
  portfolio: PortfolioResponse
  holdings?: Holding[]
}

export function PortfolioCharts({ portfolio, holdings = [] }: PortfolioChartsProps) {
  const byTypeData = portfolio.by_type.map((item) => ({
    name: item.label.charAt(0).toUpperCase() + item.label.slice(1),
    value: parseFloat(item.value),
  }))
  const byTypeColors = portfolio.by_type.map(
    (item) => ACCOUNT_TYPE_COLORS[item.label as keyof typeof ACCOUNT_TYPE_COLORS] ?? "#78716c"
  )

  const byInstData = portfolio.by_institution.map((item) => ({
    name: item.label,
    value: parseFloat(item.value),
  }))

  const byAssetClassData = portfolio.by_asset_class.map((item) => ({
    name: item.label,
    value: parseFloat(item.value),
  }))

  // Stocks breakdown (aggregate by short_name)
  const holdingsByName = new Map<string, number>()
  for (const h of holdings) {
    const key = h.short_name ?? h.symbol
    holdingsByName.set(key, (holdingsByName.get(key) ?? 0) + parseFloat(h.value))
  }
  const byStockData = Array.from(holdingsByName.entries())
    .map(([name, value]) => ({ name, value: parseFloat(value.toFixed(2)) }))
    .sort((a, b) => b.value - a.value)

  const totalStr = formatCurrency(portfolio.net_worth)
  const stocksTotal = formatCurrency(byStockData.reduce((s, d) => s + d.value, 0).toFixed(2))

  // All four charts would render empty - surface a single explanation
  // instead of four blank donuts.
  const isEmpty =
    byTypeData.length === 0 &&
    byInstData.length === 0 &&
    byAssetClassData.length === 0 &&
    byStockData.length === 0
  if (isEmpty) {
    return <EmptyState />
  }

  return (
    <div className="grid gap-4 md:grid-cols-2">
      <div className="rounded-lg border p-4">
        <h3 className="mb-2 text-sm font-medium text-muted-foreground">By Account Type</h3>
        <InteractivePie data={byTypeData} colors={byTypeColors} label={totalStr} height={260} />
      </div>
      <div className="rounded-lg border p-4">
        <h3 className="mb-2 text-sm font-medium text-muted-foreground">By Institution</h3>
        <InteractivePie data={byInstData} colors={INST_COLORS} label={totalStr} height={260} />
      </div>
      <div className="rounded-lg border p-4">
        <h3 className="mb-2 text-sm font-medium text-muted-foreground">By Asset Class</h3>
        <InteractivePie data={byAssetClassData} colors={ASSET_CLASS_COLORS} label={totalStr} height={260} />
      </div>
      {byStockData.length > 0 && (
        <div className="rounded-lg border p-4">
          <h3 className="mb-2 text-sm font-medium text-muted-foreground">By Stock</h3>
          <InteractivePie data={byStockData} colors={STOCK_COLORS} label={stocksTotal} height={260} />
        </div>
      )}
    </div>
  )
}
