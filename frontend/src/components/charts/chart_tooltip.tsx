import type { TooltipProps } from "recharts"
import { formatCurrency } from "@/lib/utils"

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
 */
export function PieTooltip({
  active,
  payload,
}: TooltipProps<number, string>) {
  if (!active || !payload || payload.length === 0) return null

  const entry = payload[0]
  const value = entry.value as number
  // Compute percentage from all pie data
  const allData = entry.payload?.payload
  let percent = 0
  if (allData && Array.isArray(allData)) {
    const total = allData.reduce((s: number, d: { value: number }) => s + d.value, 0)
    percent = total > 0 ? (value / total) * 100 : 0
  } else {
    // Fallback: Recharts sometimes puts percent on the payload
    percent = (entry.payload?.percent ?? 0) * 100
  }

  return (
    <div className="rounded-lg border border-border/50 bg-popover px-3 py-2 shadow-xl">
      <div className="flex items-center gap-2 text-sm">
        <span
          className="inline-block h-2.5 w-2.5 rounded-sm shrink-0"
          style={{ backgroundColor: entry.payload?.fill }}
        />
        <span className="font-medium text-foreground">{entry.name}</span>
      </div>
      <div className="mt-1 text-sm tabular-nums">
        <span className="text-foreground font-medium">
          {formatCurrency(value.toFixed(2))}
        </span>
        <span className="text-muted-foreground ml-1.5">
          ({percent.toFixed(1)}%)
        </span>
      </div>
    </div>
  )
}
