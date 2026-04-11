import type { PortfolioResponse } from "@/types"
import { DonutChart } from "@tremor/react"
import { formatCurrency } from "@/lib/utils"
import { ChartLegend } from "@/components/chart_legend"

const TYPE_COLORS = ["blue", "green", "violet", "red", "yellow", "indigo"]
const INST_COLORS = ["blue", "orange", "green", "pink", "cyan", "yellow"]
const SECTOR_COLORS = ["blue", "orange", "green", "pink"]

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
      <ChartCard
        title="By Account Type"
        data={byTypeData}
        colors={TYPE_COLORS}
      />
      <ChartCard
        title="By Institution"
        data={byInstData}
        colors={INST_COLORS}
      />
      <ChartCard
        title="By Sector"
        data={bySectorData}
        colors={SECTOR_COLORS}
      />
    </div>
  )
}

function ChartCard({
  title,
  data,
  colors,
}: {
  title: string
  data: { name: string; value: number }[]
  colors: string[]
}) {
  const total = data.reduce((s, d) => s + d.value, 0)
  const legendItems = data.map((d, i) => ({
    name: `${d.name} (${((d.value / total) * 100).toFixed(0)}%)`,
    color: colors[i % colors.length],
  }))

  return (
    <div className="rounded-lg border p-4">
      <h3 className="mb-3 text-sm font-medium text-muted-foreground">
        {title}
      </h3>
      <DonutChart
        data={data}
        category="value"
        index="name"
        colors={colors}
        valueFormatter={(v) => formatCurrency(v.toString())}
        className="h-52"
        showLabel
        label={formatCurrency(total.toFixed(2))}
      />
      <ChartLegend items={legendItems} className="mt-3 justify-center" />
    </div>
  )
}
