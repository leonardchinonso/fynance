import { useState, useRef, useEffect, useCallback } from "react"
import type React from "react"
import {
  PieChart,
  Pie,
  Cell,
  Tooltip,
  ResponsiveContainer,
  Sector,
} from "recharts"
import type { PieSectorDataItem } from "recharts/types/polar/Pie"
import { PieTooltip, useClampedTooltipPosition } from "./chart_tooltip"
import { ChartLegend } from "@/components/chart_legend"
import { formatCurrency, cn } from "@/lib/utils"
import { useRedactedFlag } from "@/hooks/use_redacted_flag"
import { ChevronDown, ChevronUp } from "lucide-react"

const DEFAULT_COLORS = [
  "#3b82f6", "#f97316", "#22c55e", "#a855f7", "#ec4899",
  "#06b6d4", "#eab308", "#6366f1", "#14b8a6", "#ef4444",
  "#f59e0b", "#10b981",
]

export interface PieDataItem {
  name: string
  value: number
  fullName?: string
  /** Sub-items grouped into this slice (used for "Others") */
  otherItems?: { name: string; value: number }[]
}

interface InteractivePieProps {
  data: PieDataItem[]
  colors?: string[]
  /** Stable color map keyed by item name — takes precedence over positional `colors` */
  colorMap?: Map<string, string>
  /** Currency code for the active-slice center label. Defaults to GBP. */
  currency?: string
  label?: string
  height?: number
  className?: string
  innerRadius?: number
  outerRadius?: number
  /** Where to render the legend. Defaults to "bottom" (horizontal wrap). "left" renders a vertical list. */
  legendPosition?: "bottom" | "left"
  // Right-click on a slice: `index` is the hovered slice index (null if none).
  onContextMenu?: (
    e: { clientX: number; clientY: number; preventDefault: () => void },
    ctx: { index: number | null },
  ) => void
}

export function InteractivePie({
  data,
  colors = DEFAULT_COLORS,
  colorMap,
  currency,
  label,
  height = 280,
  className,
  innerRadius = 60,
  outerRadius = 100,
  legendPosition = "bottom",
  onContextMenu,
}: InteractivePieProps) {
  useRedactedFlag()
  const [activeIndex, setActiveIndex] = useState<number | undefined>(undefined)
  const containerRef = useRef<HTMLDivElement>(null)
  const chartAreaRef = useRef<HTMLDivElement>(null)
  const legendRef = useRef<HTMLDivElement>(null)
  const [canScrollUp, setCanScrollUp] = useState(false)
  const [canScrollDown, setCanScrollDown] = useState(false)

  // Clamp the tooltip to the area the mouse coords are relative to: the plot
  // area when the legend is on the left, otherwise the whole container.
  const { pos, onMouseMove, onMouseLeave } = useClampedTooltipPosition(
    legendPosition === "left" ? chartAreaRef : containerRef,
  )

  const total = data.reduce((sum, d) => sum + d.value, 0)

  const getColor = (name: string, i: number) =>
    colorMap?.get(name) ?? colors[i % colors.length]

  const legendItems = data.map((d, i) => ({
    name: `${d.name} (${total > 0 ? ((d.value / total) * 100).toFixed(0) : 0}%)`,
    color: getColor(d.name, i),
  }))

  const clearHover = useCallback(() => {
    setActiveIndex(undefined)
    onMouseLeave()
  }, [onMouseLeave])

  // Legend scroll indicators
  function updateScrollIndicators() {
    const el = legendRef.current
    if (!el) return
    setCanScrollUp(el.scrollTop > 0)
    setCanScrollDown(el.scrollTop + el.clientHeight < el.scrollHeight - 1)
  }

  useEffect(() => {
    updateScrollIndicators()
    const el = legendRef.current
    if (!el) return
    el.addEventListener("scroll", updateScrollIndicators)
    const ro = new ResizeObserver(updateScrollIndicators)
    ro.observe(el)
    return () => {
      el.removeEventListener("scroll", updateScrollIndicators)
      ro.disconnect()
    }
  }, [data])

  const chart = (
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
          activeShape={(p: PieSectorDataItem) => renderActiveShape(p, currency)}
          onMouseEnter={(_, index) => setActiveIndex(index)}
          onMouseLeave={clearHover}
          onClick={undefined}
          onMouseDown={(e) => e.preventDefault()}
          animationBegin={0}
          animationDuration={400}
          animationEasing="ease-out"
        >
          {data.map((d, i) => (
            <Cell
              key={d.name}
              fill={getColor(d.name, i)}
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
          content={<PieTooltip total={total} data={data} />}
          position={pos}
          wrapperStyle={{ pointerEvents: "none", zIndex: 50, transition: "transform 50ms ease-out, left 50ms ease-out, top 50ms ease-out" }}
          isAnimationActive={false}
        />
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
  )

  if (legendPosition === "left") {
    return (
      <div className={cn("flex w-full", className)} ref={containerRef} onMouseMove={onMouseMove} onContextMenu={(e) => onContextMenu?.(e, { index: activeIndex ?? null })}>
        {/* Legend column */}
        <div className="shrink-0 flex flex-col min-w-0">
          {/* Up arrow — hover to scroll up continuously */}
          <div className="h-6 flex items-center justify-center shrink-0">
            {canScrollUp && (
              <ScrollArrow
                direction="up"
                legendRef={legendRef}
              />
            )}
          </div>
          {/* Scrollable list with fade masks */}
          <div className="relative flex-1 min-h-0">
            {/* Top fade mask */}
            {canScrollUp && (
              <div className="absolute top-0 left-0 right-0 h-6 bg-gradient-to-b from-card to-transparent pointer-events-none z-10" />
            )}
            <div
              ref={legendRef}
              className="overflow-y-auto h-full px-4 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
              onScroll={updateScrollIndicators}
            >
              <ChartLegend items={legendItems} className="flex-col items-start gap-y-2.5" />
            </div>
            {/* Bottom fade mask */}
            {canScrollDown && (
              <div className="absolute bottom-0 left-0 right-0 h-6 bg-gradient-to-t from-card to-transparent pointer-events-none z-10" />
            )}
          </div>
          {/* Down arrow — hover to scroll down continuously */}
          <div className="h-6 flex items-center justify-center shrink-0">
            {canScrollDown && (
              <ScrollArrow
                direction="down"
                legendRef={legendRef}
              />
            )}
          </div>
        </div>
        {/* Chart area — mouse position computed relative to this */}
        <div className="flex-1 min-w-0 flex items-center" ref={chartAreaRef}>
          {chart}
        </div>
      </div>
    )
  }

  return (
    <div className={className} ref={containerRef} onMouseMove={onMouseMove} onContextMenu={(e) => onContextMenu?.(e, { index: activeIndex ?? null })}>
      {chart}
      <ChartLegend items={legendItems} className="mt-2 justify-center" />
    </div>
  )
}

function ScrollArrow({
  direction,
  legendRef,
}: {
  direction: "up" | "down"
  legendRef: React.RefObject<HTMLDivElement | null>
}) {
  const rafRef = useRef<number | null>(null)

  const startScrolling = useCallback(() => {
    let accumulator = 0
    const step = () => {
      const el = legendRef.current
      if (!el) return
      accumulator += direction === "down" ? 0.25 : -0.25
      const wholePx = Math.trunc(accumulator)
      if (wholePx !== 0) {
        el.scrollTop += wholePx
        accumulator -= wholePx
      }
      rafRef.current = requestAnimationFrame(step)
    }
    rafRef.current = requestAnimationFrame(step)
  }, [direction, legendRef])

  const stopScrolling = useCallback(() => {
    if (rafRef.current !== null) {
      cancelAnimationFrame(rafRef.current)
      rafRef.current = null
    }
  }, [])

  useEffect(() => () => stopScrolling(), [stopScrolling])

  const Icon = direction === "up" ? ChevronUp : ChevronDown

  return (
    <div
      onMouseEnter={startScrolling}
      onMouseLeave={stopScrolling}
      className="flex items-center justify-center w-full cursor-default text-muted-foreground hover:text-foreground transition-colors"
    >
      <Icon className="h-4 w-4" strokeWidth={2.5} />
    </div>
  )
}

function renderActiveShape(props: PieSectorDataItem, currency?: string) {
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
        {formatCurrency(((props.value as number) ?? 0).toFixed(2), currency)} ({((percent ?? 0) * 100).toFixed(1)}%)
      </text>
    </g>
  )
}
