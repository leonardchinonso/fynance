import { useRef } from "react"
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
import { ChartTooltip, useClampedTooltipPosition } from "./chart_tooltip"
import { formatCurrencyCompact } from "@/lib/utils"
import { useRedactedFlag } from "@/hooks/use_redacted_flag"

const DEFAULT_COLORS = [
  "#3b82f6", "#f97316", "#22c55e", "#a855f7", "#ec4899",
  "#06b6d4", "#eab308", "#6366f1",
]

interface StyledLineChartProps {
  // `null` marks a gap (no data this period) so the line breaks instead of
  // dropping to 0 — used with the default `connectNulls={false}`.
  data: Record<string, string | number | null>[]
  index: string
  categories: string[]
  colors?: string[]
  height?: number
  className?: string
  curved?: boolean
  showLegend?: boolean
  connectNulls?: boolean
  // Categories rendered as a dashed line (e.g. an aggregate "Total" overlay).
  dashedKeys?: string[]
  showBrush?: boolean
  highlightIndex?: number | null
  onBrushChange?: (startIndex: number, endIndex: number) => void
  onActiveIndexChange?: (index: number | null) => void
  // Right-click on the plot: `index` is the hovered period (x) index, or null.
  onContextMenu?: (
    e: { clientX: number; clientY: number; preventDefault: () => void },
    ctx: { index: number | null },
  ) => void
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
  dashedKeys = [],
  showBrush = false,
  highlightIndex,
  onBrushChange,
  onActiveIndexChange,
  onContextMenu,
}: StyledLineChartProps) {
  useRedactedFlag()
  const containerRef = useRef<HTMLDivElement>(null)
  const activeIndexRef = useRef<number | null>(null)
  const { pos, onMouseMove, onMouseLeave } = useClampedTooltipPosition(containerRef)

  const highlightLabel =
    highlightIndex !== null && highlightIndex !== undefined
      ? (data[highlightIndex]?.[index] as string)
      : undefined

  return (
    <div
      className={className}
      ref={containerRef}
      onMouseMove={onMouseMove}
      onMouseLeave={onMouseLeave}
      onMouseDown={(e) => e.preventDefault()}
      onContextMenu={(e) => onContextMenu?.(e, { index: activeIndexRef.current })}
    >
      <ResponsiveContainer width="100%" height={height + (showBrush ? 40 : 0)}>
        <LineChart
          data={data}
          margin={{ top: 8, right: 32, bottom: 0, left: 16 }}
          onMouseMove={(state) => {
            if (state?.activeTooltipIndex !== undefined) {
              activeIndexRef.current = state.activeTooltipIndex
              onActiveIndexChange?.(state.activeTooltipIndex)
            }
          }}
          onMouseLeave={() => { activeIndexRef.current = null; onActiveIndexChange?.(null) }}
        >
          <CartesianGrid strokeDasharray="3 3" vertical={false} className="stroke-border/50" />
          <XAxis dataKey={index} tick={{ fontSize: 12 }} className="fill-muted-foreground text-xs" tickLine={false} axisLine={false} />
          <YAxis width={64} tick={{ fontSize: 12 }} className="fill-muted-foreground text-xs" tickLine={false} axisLine={false} tickFormatter={(v) => formatCurrencyCompact(v)} />
          <Tooltip
            content={<ChartTooltip />}
            position={pos}
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
          {categories.map((cat, i) => {
            const dashed = dashedKeys.includes(cat)
            return (
              <Line
                key={cat}
                type={curved ? "monotone" : "linear"}
                dataKey={cat}
                stroke={colors[i % colors.length]}
                strokeWidth={dashed ? 2 : 2.5}
                strokeDasharray={dashed ? "6 4" : undefined}
                dot={dashed ? false : { r: 3, fill: colors[i % colors.length], strokeWidth: 0 }}
                activeDot={{ r: 5, strokeWidth: 2, stroke: "#fff" }}
                isAnimationActive={false}
                connectNulls={connectNulls}
              />
            )
          })}
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
