import type { PortfolioResponse } from "@/types"
import { DonutChart } from "@tremor/react"
import { formatCurrency } from "@/lib/utils"

interface PortfolioChartsProps {
  portfolio: PortfolioResponse
}

export function PortfolioCharts({ portfolio }: PortfolioChartsProps) {
  const byTypeData = portfolio.by_type.map((item) => ({
    name: item.label.charAt(0).toUpperCase() + item.label.slice(1),
    value: parseFloat(item.total),
  }))

  const byInstData = portfolio.by_institution.map((item) => ({
    name: item.label,
    value: parseFloat(item.total),
  }))

  const bySectorData = portfolio.by_sector.map((item) => ({
    name: item.label,
    value: parseFloat(item.total),
  }))

  return (
    <div className="grid gap-4 md:grid-cols-3">
      <div className="rounded-lg border p-4">
        <h3 className="mb-4 text-sm font-medium text-muted-foreground">
          By Account Type
        </h3>
        <DonutChart
          data={byTypeData}
          category="value"
          index="name"
          colors={["blue", "green", "violet", "red", "yellow", "indigo"]}
          valueFormatter={(v) => formatCurrency(v.toString())}
          className="h-64"
          showLabel
        />
      </div>
      <div className="rounded-lg border p-4">
        <h3 className="mb-4 text-sm font-medium text-muted-foreground">
          By Institution
        </h3>
        <DonutChart
          data={byInstData}
          category="value"
          index="name"
          colors={["blue", "orange", "green", "pink", "cyan", "yellow"]}
          valueFormatter={(v) => formatCurrency(v.toString())}
          className="h-64"
          showLabel
        />
      </div>
      <div className="rounded-lg border p-4">
        <h3 className="mb-4 text-sm font-medium text-muted-foreground">
          By Sector
        </h3>
        <DonutChart
          data={bySectorData}
          category="value"
          index="name"
          colors={["blue", "orange", "green", "pink"]}
          valueFormatter={(v) => formatCurrency(v.toString())}
          className="h-64"
          showLabel
        />
      </div>
    </div>
  )
}
