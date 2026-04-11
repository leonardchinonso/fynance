import {
  BarChart,
  Bar,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
  Cell,
  Legend,
} from "recharts"
import { ChartTooltip } from "./chart_tooltip"
import { formatCurrency } from "@/lib/utils"

interface StyledBarChartProps {
  data: Record<string, string | number>[]
  index: string
  categories: string[]
  colors?: string[]
  stack?: boolean
  height?: number
  className?: string
  showLegend?: boolean
}

const DEFAULT_COLORS = [
  "#3b82f6", "#f97316", "#22c55e", "#a855f7", "#ec4899",
  "#06b6d4", "#eab308", "#6366f1", "#14b8a6", "#ef4444",
  "#f59e0b", "#10b981",
]

/**
 * Styled Recharts BarChart with Tremor-like visual design.
 * Supports per-category colors and stacked mode.
 */
export function StyledBarChart({
  data,
  index,
  categories,
  colors = DEFAULT_COLORS,
  stack = false,
  height = 320,
  className,
  showLegend = true,
}: StyledBarChartProps) {
  return (
    <div className={className}>
      <ResponsiveContainer width="100%" height={height}>
        <BarChart data={data} margin={{ top: 8, right: 8, bottom: 0, left: 12 }}>
          <CartesianGrid
            strokeDasharray="3 3"
            vertical={false}
            className="stroke-border/50"
          />
          <XAxis
            dataKey={index}
            tick={{ fontSize: 12 }}
            className="fill-muted-foreground text-xs"
            tickLine={false}
            axisLine={false}
          />
          <YAxis
            tick={{ fontSize: 12 }}
            className="fill-muted-foreground text-xs"
            tickLine={false}
            axisLine={false}
            tickFormatter={(v) => formatCurrency(v.toString())}
          />
          <Tooltip
            content={<ChartTooltip />}
            cursor={{ fill: "hsl(var(--muted))", opacity: 0.3 }}
          />
          {showLegend && categories.length > 1 && (
            <Legend
              wrapperStyle={{ fontSize: "12px", paddingTop: "12px" }}
              formatter={(value) => (
                <span className="text-muted-foreground text-xs">{value}</span>
              )}
            />
          )}
          {categories.map((cat, i) => (
            <Bar
              key={cat}
              dataKey={cat}
              fill={colors[i % colors.length]}
              radius={stack ? [0, 0, 0, 0] : [4, 4, 0, 0]}
              stackId={stack ? "stack" : undefined}
              animationDuration={400}
              animationEasing="ease-out"
            />
          ))}
        </BarChart>
      </ResponsiveContainer>
    </div>
  )
}

/**
 * Single-category bar chart with per-bar colors (each bar a different color).
 */
export function ColoredBarChart({
  data,
  index,
  valueKey,
  colors = DEFAULT_COLORS,
  height = 320,
  className,
}: {
  data: Record<string, string | number>[]
  index: string
  valueKey: string
  colors?: string[]
  height?: number
  className?: string
}) {
  return (
    <div className={className}>
      <ResponsiveContainer width="100%" height={height}>
        <BarChart data={data} margin={{ top: 8, right: 8, bottom: 0, left: 12 }}>
          <CartesianGrid
            strokeDasharray="3 3"
            vertical={false}
            className="stroke-border/50"
          />
          <XAxis
            dataKey={index}
            tick={{ fontSize: 12 }}
            className="fill-muted-foreground text-xs"
            tickLine={false}
            axisLine={false}
          />
          <YAxis
            tick={{ fontSize: 12 }}
            className="fill-muted-foreground text-xs"
            tickLine={false}
            axisLine={false}
            tickFormatter={(v) => formatCurrency(v.toString())}
          />
          <Tooltip
            content={<ChartTooltip />}
            cursor={{ fill: "hsl(var(--muted))", opacity: 0.3 }}
          />
          <Bar dataKey={valueKey} radius={[4, 4, 0, 0]} animationDuration={400}>
            {data.map((_, i) => (
              <Cell key={i} fill={colors[i % colors.length]} />
            ))}
          </Bar>
        </BarChart>
      </ResponsiveContainer>
    </div>
  )
}
