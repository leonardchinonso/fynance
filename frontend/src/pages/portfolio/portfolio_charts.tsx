import type { PortfolioResponse } from "@/types"
import { InteractivePie } from "@/components/charts"
import { ACCOUNT_TYPE_COLORS } from "@/lib/colors"
import { formatCurrency } from "@/lib/utils"

const INST_COLORS = ["#3b82f6", "#f97316", "#22c55e", "#ec4899", "#06b6d4", "#eab308", "#6366f1"]
const SECTOR_COLORS = ["#3b82f6", "#f97316", "#22c55e", "#ec4899", "#a855f7"]

interface PortfolioChartsProps {
  portfolio: PortfolioResponse
}

export function PortfolioCharts({ portfolio }: PortfolioChartsProps) {
  const byTypeData = portfolio.by_type.map((item) => ({
    name: item.label.charAt(0).toUpperCase() + item.label.slice(1),
    value: parseFloat(item.total),
  }))
  const byTypeColors = portfolio.by_type.map(
    (item) => ACCOUNT_TYPE_COLORS[item.label as keyof typeof ACCOUNT_TYPE_COLORS] ?? "#78716c"
  )

  const byInstData = portfolio.by_institution.map((item) => ({
    name: item.label,
    value: parseFloat(item.total),
  }))

  const bySectorData = portfolio.by_sector.map((item) => ({
    name: item.label,
    value: parseFloat(item.total),
  }))

  const totalStr = formatCurrency(portfolio.net_worth)

  return (
    <div className="grid gap-4 md:grid-cols-3">
      <div className="rounded-lg border p-4">
        <h3 className="mb-2 text-sm font-medium text-muted-foreground">
          By Account Type
        </h3>
        <InteractivePie
          data={byTypeData}
          colors={byTypeColors}
          label={totalStr}
          height={280}
        />
      </div>
      <div className="rounded-lg border p-4">
        <h3 className="mb-2 text-sm font-medium text-muted-foreground">
          By Institution
        </h3>
        <InteractivePie
          data={byInstData}
          colors={INST_COLORS}
          label={totalStr}
          height={280}
        />
      </div>
      <div className="rounded-lg border p-4">
        <h3 className="mb-2 text-sm font-medium text-muted-foreground">
          By Sector
        </h3>
        <InteractivePie
          data={bySectorData}
          colors={SECTOR_COLORS}
          label={totalStr}
          height={280}
        />
      </div>
    </div>
  )
}
