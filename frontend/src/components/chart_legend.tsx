import { cn } from "@/lib/utils"

const COLOR_MAP: Record<string, string> = {
  blue: "#3b82f6",
  green: "#22c55e",
  violet: "#a855f7",
  red: "#ef4444",
  yellow: "#eab308",
  indigo: "#6366f1",
  orange: "#f97316",
  pink: "#ec4899",
  cyan: "#06b6d4",
  teal: "#14b8a6",
  emerald: "#10b981",
  amber: "#f59e0b",
}

interface ChartLegendProps {
  items: { name: string; color: string }[]
  className?: string
}

export function ChartLegend({ items, className }: ChartLegendProps) {
  return (
    <div className={cn("flex flex-wrap items-center gap-x-4 gap-y-1.5 text-xs", className)}>
      {items.map((item) => (
        <div key={item.name} className="flex items-center gap-1.5">
          <span
            className="inline-block h-2.5 w-2.5 rounded-sm shrink-0"
            style={{ backgroundColor: COLOR_MAP[item.color] ?? item.color }}
          />
          <span className="text-muted-foreground">{item.name}</span>
        </div>
      ))}
    </div>
  )
}
