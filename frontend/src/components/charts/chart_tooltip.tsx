import { useCallback, useLayoutEffect, useRef, useState } from "react"
import type React from "react"
import type { TooltipProps } from "recharts"
import { formatCurrency } from "@/lib/utils"
import type { PieDataItem } from "./interactive_pie"

const CURSOR_OFFSET = 15 // gap between the cursor and the tooltip
const EDGE_MARGIN = 8 // keep the tooltip this far inside the container edges

/**
 * Positions a Recharts custom tooltip so it tracks the cursor but never escapes
 * its chart container: it flips to the left of the cursor near the right edge,
 * and is clamped to the container box on every side. Shared by every chart so
 * the behaviour is defined once instead of duplicated per chart.
 *
 * Pass the element the tooltip must stay inside (the chart's container, or just
 * the plot area for charts with a side legend). Spread the returned handlers on
 * that hover target and pass `pos` to `<Tooltip position={pos} />`.
 */
export function useClampedTooltipPosition(
  containerRef: React.RefObject<HTMLElement | null>,
) {
  const rawRef = useRef<{ x: number; y: number } | null>(null)
  const [pos, setPos] = useState<{ x: number; y: number } | undefined>(undefined)

  const recompute = useCallback(() => {
    const container = containerRef.current
    const raw = rawRef.current
    if (!container || !raw) return
    const wrapper = container.querySelector<HTMLElement>(".recharts-tooltip-wrapper")
    const tw = wrapper?.offsetWidth ?? 0
    const th = wrapper?.offsetHeight ?? 0
    const cw = container.clientWidth
    const ch = container.clientHeight

    let x = raw.x + CURSOR_OFFSET
    // Flip to the left of the cursor when the tooltip would spill past the right edge.
    if (tw > 0 && x + tw > cw - EDGE_MARGIN) x = raw.x - CURSOR_OFFSET - tw
    if (tw > 0) x = Math.min(Math.max(EDGE_MARGIN, x), Math.max(EDGE_MARGIN, cw - tw - EDGE_MARGIN))

    let y = raw.y + CURSOR_OFFSET
    if (th > 0) y = Math.min(Math.max(EDGE_MARGIN, y), Math.max(EDGE_MARGIN, ch - th - EDGE_MARGIN))

    setPos((prev) =>
      prev && Math.abs(prev.x - x) < 0.5 && Math.abs(prev.y - y) < 0.5 ? prev : { x, y },
    )
  }, [containerRef])

  const onMouseMove = useCallback(
    (e: React.MouseEvent) => {
      const container = containerRef.current
      if (!container) return
      const rect = container.getBoundingClientRect()
      rawRef.current = { x: e.clientX - rect.left, y: e.clientY - rect.top }
      recompute()
    },
    [containerRef, recompute],
  )

  const onMouseLeave = useCallback(() => {
    rawRef.current = null
    setPos(undefined)
  }, [])

  // The tooltip's size is only known once it has rendered, so re-clamp after
  // each render (cheap, and guarded so it can't loop).
  useLayoutEffect(() => {
    if (rawRef.current) recompute()
  })

  return { pos, onMouseMove, onMouseLeave }
}

/**
 * Styled tooltip matching Tremor's visual design.
 * Used across all Recharts charts for consistent look.
 */
export function ChartTooltip({
  active,
  payload,
  label,
  formatter,
  activeCategory,
  showTotal,
}: TooltipProps<number, string> & {
  formatter?: (value: number, name: string) => string
  activeCategory?: string | null
  showTotal?: boolean
}) {
  if (!active || !payload || payload.length === 0) return null

  // Skip series with no value in this period (gaps before tracking starts) so the
  // tooltip doesn't try to render null entries.
  const visible = payload.filter((entry) => entry.value != null)
  if (visible.length === 0) return null

  const total = visible.reduce((sum, entry) => sum + (entry.value as number), 0)

  // Sort: active category first, then by original order
  const sorted = activeCategory
    ? [...visible].sort((a, b) => {
        if (a.name === activeCategory) return -1
        if (b.name === activeCategory) return 1
        return 0
      })
    : visible

  return (
    <div className="rounded-lg border border-border/50 bg-popover px-3 py-2 shadow-xl">
      {label && (
        <p className="mb-1.5 text-xs font-medium text-muted-foreground">
          {label}
        </p>
      )}
      <div className="space-y-1">
        {sorted.map((entry, i) => {
          const isActive = activeCategory === entry.name
          return (
            <div key={i} className="flex items-center gap-2 text-sm">
              <span
                className="inline-block h-2.5 w-2.5 rounded-sm shrink-0"
                style={{ backgroundColor: entry.color }}
              />
              <span className={isActive ? "text-foreground font-semibold" : "text-muted-foreground"}>
                {entry.name}
              </span>
              <span className={`ml-auto tabular-nums ${isActive ? "font-bold text-foreground" : "font-medium text-foreground"}`}>
                {formatter
                  ? formatter(entry.value as number, entry.name as string)
                  : formatCurrency((entry.value as number).toFixed(2))}
              </span>
            </div>
          )
        })}
      </div>
      {showTotal && (
        <div className="mt-1.5 flex items-center gap-2 border-t border-border/40 pt-1.5 text-sm">
          <span className="inline-block h-2.5 w-2.5 shrink-0" />
          <span className="font-semibold text-foreground">Total</span>
          <span className="ml-auto tabular-nums font-bold text-foreground">
            {formatter ? formatter(total, "Total") : formatCurrency(total.toFixed(2))}
          </span>
        </div>
      )}
    </div>
  )
}

/**
 * Tooltip for pie/donut charts showing percentage.
 * `total` is passed from InteractivePie since Recharts does not include percent in tooltipPayload items.
 * `data` is passed so we can look up `otherItems` for the "Others" grouped slice.
 */
export function PieTooltip({
  active,
  payload,
  total,
  data,
}: TooltipProps<number, string> & { total?: number; data?: PieDataItem[] }) {
  if (!active || !payload || payload.length === 0) return null

  const entry = payload[0]
  const value = entry.value as number
  const percent = total && total > 0 ? (value / total) * 100 : 0
  const fullName: string | undefined = entry.payload?.fullName
  const color = (entry as { fill?: string }).fill ?? entry.payload?.fill

  // Find otherItems from the data array by matching name
  const dataItem = data?.find(d => d.name === entry.name)
  const otherItems = dataItem?.otherItems

  return (
    <div className="rounded-lg border border-border/50 bg-popover px-3 py-2 shadow-xl max-w-56">
      <div className="flex items-center gap-2 text-sm">
        <span
          className="inline-block h-2.5 w-2.5 rounded-sm shrink-0"
          style={{ backgroundColor: color }}
        />
        <span className="font-medium text-foreground">{fullName ?? entry.name}</span>
      </div>
      <div className="mt-1 text-sm tabular-nums">
        <span className="text-foreground font-medium">
          {formatCurrency(value.toFixed(2))}
        </span>
        <span className="text-muted-foreground ml-1.5">
          ({percent.toFixed(1)}%)
        </span>
      </div>
      {otherItems && otherItems.length > 0 && (
        <div className="mt-2 border-t border-border/40 pt-2 space-y-1">
          {otherItems.map((item, i) => (
            <div key={i} className="flex justify-between gap-3 text-xs text-muted-foreground">
              <span className="truncate">{item.name}</span>
              <span className="tabular-nums shrink-0">{formatCurrency(item.value.toFixed(2))}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  )
}
