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
}: TooltipProps<number, string> & {
  formatter?: (value: number, name: string) => string
}) {
  if (!active || !payload || payload.length === 0) return null

  return (
    <div className="rounded-lg border border-border/50 bg-popover px-3 py-2 shadow-xl">
      {label && (
        <p className="mb-1.5 text-xs font-medium text-muted-foreground">
          {label}
        </p>
      )}
      <div className="space-y-1">
        {payload.map((entry, i) => (
          <div key={i} className="flex items-center gap-2 text-sm">
            <span
              className="inline-block h-2.5 w-2.5 rounded-sm shrink-0"
              style={{ backgroundColor: entry.color }}
            />
            <span className="text-muted-foreground">{entry.name}</span>
            <span className="ml-auto font-medium text-foreground tabular-nums">
              {formatter
                ? formatter(entry.value as number, entry.name as string)
                : formatCurrency((entry.value as number).toFixed(2))}
            </span>
          </div>
        ))}
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
  const data = entry.payload
  const percent = data?.percent ?? 0

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
          {formatCurrency((entry.value as number).toFixed(2))}
        </span>
        <span className="text-muted-foreground ml-1.5">
          ({(percent * 100).toFixed(1)}%)
        </span>
      </div>
    </div>
  )
}
