import type { TooltipProps } from "recharts"
import { formatCurrency } from "@/lib/utils"
import type { PieDataItem } from "./interactive_pie"

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
}: TooltipProps<number, string> & {
  formatter?: (value: number, name: string) => string
  activeCategory?: string | null
}) {
  if (!active || !payload || payload.length === 0) return null

  // Sort: active category first, then by original order
  const sorted = activeCategory
    ? [...payload].sort((a, b) => {
        if (a.name === activeCategory) return -1
        if (b.name === activeCategory) return 1
        return 0
      })
    : payload

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
