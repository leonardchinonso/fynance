import {
  LineChart,
  Line,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
  Legend,
  Brush,
  ReferenceLine,
} from "recharts"
import { ChartTooltip } from "./chart_tooltip"
import { formatCurrency } from "@/lib/utils"

const DEFAULT_COLORS = [
  "#3b82f6", "#f97316", "#22c55e", "#a855f7", "#ec4899",
  "#06b6d4", "#eab308", "#6366f1",
]

interface StyledLineChartProps {
  data: Record<string, string | number>[]
  index: string
  categories: string[]
  colors?: string[]
  height?: number
  className?: string
  curved?: boolean
  showLegend?: boolean
  connectNulls?: boolean
  showBrush?: boolean
  highlightIndex?: number | null
  onBrushChange?: (startIndex: number, endIndex: number) => void
}

/**
 * Styled Recharts LineChart with smooth curves, optional brush for
 * range selection, and highlighted index for table-chart sync.
 */
export function StyledLineChart({
  data,
  index,
  categories,
  colors = DEFAULT_COLORS,
  height = 320,
  className,
  curved = true,
  showLegend = true,
  connectNulls = false,
  showBrush = false,
  highlightIndex,
  onBrushChange,
}: StyledLineChartProps) {
  const highlightLabel =
    highlightIndex !== null && highlightIndex !== undefined
      ? (data[highlightIndex]?.[index] as string)
      : undefined

  return (
    <div className={className}>
      <ResponsiveContainer width="100%" height={height + (showBrush ? 40 : 0)}>
        <LineChart data={data} margin={{ top: 8, right: 8, bottom: 0, left: 0 }}>
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
          <Tooltip content={<ChartTooltip />} />
          {showLegend && categories.length > 1 && (
            <Legend
              wrapperStyle={{ fontSize: "12px", paddingTop: "12px" }}
              formatter={(value) => (
                <span className="text-muted-foreground text-xs">{value}</span>
              )}
            />
          )}
          {/* Highlight line for table hover sync */}
          {highlightLabel && (
            <ReferenceLine
              x={highlightLabel}
              stroke="hsl(var(--foreground))"
              strokeWidth={1.5}
              strokeDasharray="4 4"
              opacity={0.5}
            />
          )}
          {categories.map((cat, i) => (
            <Line
              key={cat}
              type={curved ? "monotone" : "linear"}
              dataKey={cat}
              stroke={colors[i % colors.length]}
              strokeWidth={2.5}
              dot={{ r: 3, fill: colors[i % colors.length], strokeWidth: 0 }}
              activeDot={{ r: 5, strokeWidth: 2, stroke: "#fff" }}
              animationDuration={500}
              animationEasing="ease-out"
              connectNulls={connectNulls}
            />
          ))}
          {showBrush && (
            <Brush
              dataKey={index}
              height={28}
              stroke="hsl(var(--border))"
              fill="hsl(var(--muted))"
              travellerWidth={8}
              onChange={(range) => {
                if (onBrushChange && range.startIndex !== undefined && range.endIndex !== undefined) {
                  onBrushChange(range.startIndex, range.endIndex)
                }
              }}
            >
              {/* Mini chart inside the brush */}
              <LineChart data={data}>
                {categories.slice(0, 1).map((cat, i) => (
                  <Line
                    key={cat}
                    type="monotone"
                    dataKey={cat}
                    stroke={colors[i % colors.length]}
                    strokeWidth={1}
                    dot={false}
                  />
                ))}
              </LineChart>
            </Brush>
          )}
        </LineChart>
      </ResponsiveContainer>
    </div>
  )
}
