import { useState, useRef } from "react"
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
  onActiveIndexChange?: (index: number | null) => void
}

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
  onActiveIndexChange,
}: StyledLineChartProps) {
  const [mousePos, setMousePos] = useState<{ x: number; y: number } | null>(null)
  const containerRef = useRef<HTMLDivElement>(null)

  function handleMouseMove(e: React.MouseEvent) {
    if (!containerRef.current) return
    const rect = containerRef.current.getBoundingClientRect()
    setMousePos({ x: e.clientX - rect.left + 15, y: e.clientY - rect.top + 15 })
  }

  const highlightLabel =
    highlightIndex !== null && highlightIndex !== undefined
      ? (data[highlightIndex]?.[index] as string)
      : undefined

  return (
    <div className={className} ref={containerRef} onMouseMove={handleMouseMove} onMouseLeave={() => setMousePos(null)} onMouseDown={(e) => e.preventDefault()}>
      <ResponsiveContainer width="100%" height={height + (showBrush ? 40 : 0)}>
        <LineChart
          data={data}
          margin={{ top: 8, right: 16, bottom: 0, left: 16 }}
          onMouseMove={(state) => {
            if (onActiveIndexChange && state?.activeTooltipIndex !== undefined) {
              onActiveIndexChange(state.activeTooltipIndex)
            }
          }}
          onMouseLeave={() => onActiveIndexChange?.(null)}
        >
          <CartesianGrid strokeDasharray="3 3" vertical={false} className="stroke-border/50" />
          <XAxis dataKey={index} tick={{ fontSize: 12 }} className="fill-muted-foreground text-xs" tickLine={false} axisLine={false} />
          <YAxis tick={{ fontSize: 12 }} className="fill-muted-foreground text-xs" tickLine={false} axisLine={false} tickFormatter={(v) => formatCurrency(v.toString())} />
          <Tooltip
            content={<ChartTooltip />}
            position={mousePos ?? undefined}
            wrapperStyle={{ pointerEvents: "none", zIndex: 50, transition: "transform 50ms ease-out, left 50ms ease-out, top 50ms ease-out" }}
            isAnimationActive={false}
          />
          {showLegend && categories.length > 1 && (
            <Legend
              wrapperStyle={{ fontSize: "12px", paddingTop: "12px" }}
              formatter={(value) => <span className="text-muted-foreground text-xs">{value}</span>}
            />
          )}
          {highlightLabel && (
            <ReferenceLine x={highlightLabel} stroke="#ffffff" strokeWidth={2} strokeDasharray="4 4" opacity={0.6} />
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
              isAnimationActive={false}
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
              <LineChart data={data}>
                {categories.slice(0, 1).map((cat, i) => (
                  <Line key={cat} type="monotone" dataKey={cat} stroke={colors[i % colors.length]} strokeWidth={1} dot={false} />
                ))}
              </LineChart>
            </Brush>
          )}
        </LineChart>
      </ResponsiveContainer>
    </div>
  )
}
