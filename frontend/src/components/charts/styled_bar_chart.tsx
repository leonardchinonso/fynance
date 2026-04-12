import { useState, useRef } from "react"
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

const DEFAULT_COLORS = [
  "#3b82f6", "#f97316", "#22c55e", "#a855f7", "#ec4899",
  "#06b6d4", "#eab308", "#6366f1", "#14b8a6", "#ef4444",
  "#f59e0b", "#10b981",
]

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
  const [mousePos, setMousePos] = useState<{ x: number; y: number } | null>(null)
  const [activeBarIndex, setActiveBarIndex] = useState<number | null>(null)
  const [activeCatIndex, setActiveCatIndex] = useState<number | null>(null)
  const containerRef = useRef<HTMLDivElement>(null)

  function handleMouseMove(e: React.MouseEvent) {
    if (!containerRef.current) return
    const rect = containerRef.current.getBoundingClientRect()
    setMousePos({ x: e.clientX - rect.left + 15, y: e.clientY - rect.top + 15 })
  }

  return (
    <div className={className} ref={containerRef} onMouseMove={handleMouseMove} onMouseLeave={() => { setMousePos(null); setActiveBarIndex(null); setActiveCatIndex(null) }} onMouseDown={(e) => e.preventDefault()}>
      <ResponsiveContainer width="100%" height={height}>
        <BarChart
          data={data}
          margin={{ top: 8, right: 16, bottom: 0, left: 16 }}
          onMouseMove={(state) => {
            if (state?.activeTooltipIndex !== undefined) {
              setActiveBarIndex(state.activeTooltipIndex)
            }
          }}
          onMouseLeave={() => { setActiveBarIndex(null); setActiveCatIndex(null) }}
        >
          <CartesianGrid strokeDasharray="3 3" vertical={false} className="stroke-border/50" />
          <XAxis dataKey={index} tick={{ fontSize: 12 }} className="fill-muted-foreground text-xs" tickLine={false} axisLine={false} />
          <YAxis tick={{ fontSize: 12 }} className="fill-muted-foreground text-xs" tickLine={false} axisLine={false} tickFormatter={(v) => formatCurrency(v.toString())} />
          <Tooltip
            content={<ChartTooltip activeCategory={activeCatIndex !== null ? categories[activeCatIndex] : null} />}
            position={mousePos ?? undefined}
            wrapperStyle={{ pointerEvents: "none", zIndex: 50, transition: "transform 50ms ease-out, left 50ms ease-out, top 50ms ease-out" }}
            isAnimationActive={false}
            cursor={false}
          />
          {showLegend && categories.length > 1 && (
            <Legend
              wrapperStyle={{ fontSize: "12px", paddingTop: "12px" }}
              formatter={(value) => <span className="text-muted-foreground text-xs">{value}</span>}
            />
          )}
          {categories.map((cat, catIdx) => (
            <Bar
              key={cat}
              dataKey={cat}
              fill={colors[catIdx % colors.length]}
              radius={stack ? [0, 0, 0, 0] : [4, 4, 0, 0]}
              stackId={stack ? "stack" : undefined}
              isAnimationActive={false}
              onMouseEnter={() => setActiveCatIndex(catIdx)}
              onMouseLeave={() => setActiveCatIndex(null)}
            >
              {data.map((_, dataIdx) => {
                const isActiveColumn = activeBarIndex === dataIdx
                const isActiveSegment = isActiveColumn && activeCatIndex === catIdx
                const baseColor = colors[catIdx % colors.length]
                return (
                  <Cell
                    key={dataIdx}
                    fill={baseColor}
                    style={{
                      filter: isActiveSegment
                        ? "brightness(1.35)"
                        : isActiveColumn
                          ? "brightness(1.1)"
                          : activeBarIndex !== null
                            ? "brightness(0.85)"
                            : "none",
                      stroke: isActiveSegment ? "rgba(255,255,255,0.4)" : "none",
                      strokeWidth: isActiveSegment ? 2 : 0,
                      transform: isActiveColumn ? "scaleY(1.03)" : "scaleY(1)",
                      transformOrigin: "center bottom",
                      transition: "filter 150ms ease-out, transform 150ms ease-out",
                      outline: "none",
                    }}
                  />
                )
              })}
            </Bar>
          ))}
        </BarChart>
      </ResponsiveContainer>
    </div>
  )
}

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
  const [mousePos, setMousePos] = useState<{ x: number; y: number } | null>(null)
  const [activeBarIndex, setActiveBarIndex] = useState<number | null>(null)
  const containerRef = useRef<HTMLDivElement>(null)

  function handleMouseMove(e: React.MouseEvent) {
    if (!containerRef.current) return
    const rect = containerRef.current.getBoundingClientRect()
    setMousePos({ x: e.clientX - rect.left + 15, y: e.clientY - rect.top + 15 })
  }

  return (
    <div className={className} ref={containerRef} onMouseMove={handleMouseMove} onMouseLeave={() => { setMousePos(null); setActiveBarIndex(null) }} onMouseDown={(e) => e.preventDefault()}>
      <ResponsiveContainer width="100%" height={height}>
        <BarChart
          data={data}
          margin={{ top: 8, right: 16, bottom: 0, left: 16 }}
          onMouseMove={(state) => {
            if (state?.activeTooltipIndex !== undefined) {
              setActiveBarIndex(state.activeTooltipIndex)
            }
          }}
          onMouseLeave={() => setActiveBarIndex(null)}
        >
          <CartesianGrid strokeDasharray="3 3" vertical={false} className="stroke-border/50" />
          <XAxis dataKey={index} tick={{ fontSize: 12 }} className="fill-muted-foreground text-xs" tickLine={false} axisLine={false} />
          <YAxis tick={{ fontSize: 12 }} className="fill-muted-foreground text-xs" tickLine={false} axisLine={false} tickFormatter={(v) => formatCurrency(v.toString())} />
          <Tooltip
            content={<ChartTooltip />}
            position={mousePos ?? undefined}
            wrapperStyle={{ pointerEvents: "none", zIndex: 50, transition: "transform 50ms ease-out, left 50ms ease-out, top 50ms ease-out" }}
            isAnimationActive={false}
            cursor={false}
          />
          <Bar dataKey={valueKey} radius={[4, 4, 0, 0]} isAnimationActive={false}>
            {data.map((_, i) => {
              const isActive = activeBarIndex === i
              return (
                <Cell
                  key={i}
                  fill={colors[i % colors.length]}
                  style={{
                    filter: isActive ? "brightness(1.25)" : activeBarIndex !== null ? "brightness(0.85)" : "none",
                    stroke: isActive ? "rgba(255,255,255,0.3)" : "none",
                    strokeWidth: isActive ? 2 : 0,
                    transition: "filter 150ms ease-out, transform 150ms ease-out",
                    outline: "none",
                  }}
                />
              )
            })}
          </Bar>
        </BarChart>
      </ResponsiveContainer>
    </div>
  )
}
