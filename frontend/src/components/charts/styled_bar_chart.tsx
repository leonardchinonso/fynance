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
  const containerRef = useRef<HTMLDivElement>(null)

  function handleMouseMove(e: React.MouseEvent) {
    if (!containerRef.current) return
    const rect = containerRef.current.getBoundingClientRect()
    setMousePos({ x: e.clientX - rect.left + 15, y: e.clientY - rect.top + 15 })
  }

  return (
    <div className={className} ref={containerRef} onMouseMove={handleMouseMove} onMouseLeave={() => setMousePos(null)}>
      <ResponsiveContainer width="100%" height={height}>
        <BarChart data={data} margin={{ top: 8, right: 8, bottom: 0, left: 12 }}>
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
  const containerRef = useRef<HTMLDivElement>(null)

  function handleMouseMove(e: React.MouseEvent) {
    if (!containerRef.current) return
    const rect = containerRef.current.getBoundingClientRect()
    setMousePos({ x: e.clientX - rect.left + 15, y: e.clientY - rect.top + 15 })
  }

  return (
    <div className={className} ref={containerRef} onMouseMove={handleMouseMove} onMouseLeave={() => setMousePos(null)}>
      <ResponsiveContainer width="100%" height={height}>
        <BarChart data={data} margin={{ top: 8, right: 8, bottom: 0, left: 12 }}>
          <CartesianGrid strokeDasharray="3 3" vertical={false} className="stroke-border/50" />
          <XAxis dataKey={index} tick={{ fontSize: 12 }} className="fill-muted-foreground text-xs" tickLine={false} axisLine={false} />
          <YAxis tick={{ fontSize: 12 }} className="fill-muted-foreground text-xs" tickLine={false} axisLine={false} tickFormatter={(v) => formatCurrency(v.toString())} />
          <Tooltip
            content={<ChartTooltip />}
            position={mousePos ?? undefined}
            wrapperStyle={{ pointerEvents: "none", zIndex: 50, transition: "transform 50ms ease-out, left 50ms ease-out, top 50ms ease-out" }}
            isAnimationActive={false}
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
