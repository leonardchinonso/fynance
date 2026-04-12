import { useState, useRef } from "react"
import {
  PieChart,
  Pie,
  Cell,
  Tooltip,
  ResponsiveContainer,
  Sector,
} from "recharts"
import type { PieSectorDataItem } from "recharts/types/polar/Pie"
import { PieTooltip } from "./chart_tooltip"
import { ChartLegend } from "@/components/chart_legend"
import { formatCurrency } from "@/lib/utils"

const DEFAULT_COLORS = [
  "#3b82f6", "#f97316", "#22c55e", "#a855f7", "#ec4899",
  "#06b6d4", "#eab308", "#6366f1", "#14b8a6", "#ef4444",
  "#f59e0b", "#10b981",
]

interface InteractivePieProps {
  data: { name: string; value: number }[]
  colors?: string[]
  label?: string
  height?: number
  className?: string
  innerRadius?: number
  outerRadius?: number
}

export function InteractivePie({
  data,
  colors = DEFAULT_COLORS,
  label,
  height = 280,
  className,
  innerRadius = 60,
  outerRadius = 100,
}: InteractivePieProps) {
  const [activeIndex, setActiveIndex] = useState<number | undefined>(undefined)
  const [mousePos, setMousePos] = useState<{ x: number; y: number } | null>(null)
  const containerRef = useRef<HTMLDivElement>(null)

  const total = data.reduce((sum, d) => sum + d.value, 0)

  const legendItems = data.map((d, i) => ({
    name: `${d.name} (${total > 0 ? ((d.value / total) * 100).toFixed(0) : 0}%)`,
    color: colors[i % colors.length],
  }))

  function handleMouseMove(e: React.MouseEvent) {
    if (!containerRef.current) return
    const rect = containerRef.current.getBoundingClientRect()
    setMousePos({
      x: e.clientX - rect.left + 15,
      y: e.clientY - rect.top + 15,
    })
  }

  return (
    <div className={className} ref={containerRef} onMouseMove={handleMouseMove}>
      <ResponsiveContainer width="100%" height={height}>
        <PieChart>
          <Pie
            data={data}
            cx="50%"
            cy="50%"
            innerRadius={innerRadius}
            outerRadius={outerRadius}
            dataKey="value"
            nameKey="name"
            activeIndex={activeIndex}
            activeShape={renderActiveShape}
            onMouseEnter={(_, index) => setActiveIndex(index)}
            onMouseLeave={() => { setActiveIndex(undefined); setMousePos(null); }}
            onClick={undefined}
            onMouseDown={(e) => e.preventDefault()}
            animationBegin={0}
            animationDuration={400}
            animationEasing="ease-out"
          >
            {data.map((_, i) => (
              <Cell
                key={i}
                fill={colors[i % colors.length]}
                stroke="transparent"
                style={{
                  outline: "none",
                  cursor: "pointer",
                  filter: activeIndex !== undefined && activeIndex !== i ? "brightness(0.85)" : "none",
                  transition: "filter 150ms ease-out",
                }}
              />
            ))}
          </Pie>
          <Tooltip
            content={<PieTooltip />}
            position={mousePos ?? undefined}
            wrapperStyle={{ pointerEvents: "none", zIndex: 50, transition: "transform 50ms ease-out, left 50ms ease-out, top 50ms ease-out" }}
            isAnimationActive={false}
          />
          {/* Center label - hidden when a segment is hovered */}
          {label && activeIndex === undefined && (
            <text
              x="50%"
              y="50%"
              textAnchor="middle"
              dominantBaseline="central"
              className="fill-foreground text-sm font-semibold"
            >
              {label}
            </text>
          )}
        </PieChart>
      </ResponsiveContainer>
      <ChartLegend items={legendItems} className="mt-2 justify-center" />
    </div>
  )
}

function renderActiveShape(props: PieSectorDataItem) {
  const {
    cx, cy, innerRadius, outerRadius, startAngle, endAngle, fill, payload, percent,
  } = props

  const or = outerRadius as number
  const ir = innerRadius as number

  return (
    <g>
      <Sector
        cx={cx}
        cy={cy}
        innerRadius={ir - 2}
        outerRadius={or + 8}
        startAngle={startAngle}
        endAngle={endAngle}
        fill={fill}
        style={{ filter: "brightness(1.15)", outline: "none" }}
      />
      <Sector
        cx={cx}
        cy={cy}
        innerRadius={or + 10}
        outerRadius={or + 14}
        startAngle={startAngle}
        endAngle={endAngle}
        fill={fill}
        opacity={0.3}
        style={{ outline: "none" }}
      />
      <text
        x={cx}
        y={(cy as number) - 8}
        textAnchor="middle"
        className="fill-foreground text-xs font-medium"
      >
        {(payload as { name?: string })?.name}
      </text>
      <text
        x={cx}
        y={(cy as number) + 10}
        textAnchor="middle"
        className="fill-muted-foreground text-xs"
      >
        {formatCurrency(((props.value as number) ?? 0).toFixed(2))} ({((percent ?? 0) * 100).toFixed(1)}%)
      </text>
    </g>
  )
}
