import { useState } from "react"
import { parse, format } from "date-fns"
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover"
import { Calendar } from "@/components/ui/calendar"
import { formatDate } from "@/lib/utils"
import { cn } from "@/lib/utils"

interface Props {
  /** ISO YYYY-MM-DD or empty string for "no bound". */
  start: string
  /** ISO YYYY-MM-DD or empty string for "no bound". */
  end: string
  onChange: (next: { start: string; end: string }) => void
  className?: string
}

/**
 * Compact date range with two clickable text triggers joined by "to", each
 * opening a Calendar popover. Matches the look of the global DateRangeSelector
 * but takes value+onChange (no URL hook), suitable for ephemeral filters
 * inside dialogs / preview panels.
 */
export function InlineDateRange({ start, end, onChange, className }: Props) {
  const [startOpen, setStartOpen] = useState(false)
  const [endOpen, setEndOpen] = useState(false)

  const startDate = start ? parse(start, "yyyy-MM-dd", new Date()) : undefined
  const endDate = end ? parse(end, "yyyy-MM-dd", new Date()) : undefined

  return (
    <div className={cn("flex items-center gap-1 text-sm text-muted-foreground", className)}>
      <Popover open={startOpen} onOpenChange={setStartOpen}>
        <PopoverTrigger className="hover:text-foreground hover:underline transition-colors cursor-pointer tabular-nums">
          {start ? formatDate(start) : "Any"}
        </PopoverTrigger>
        <PopoverContent className="w-auto p-0" align="start">
          <Calendar
            mode="single"
            selected={startDate}
            onSelect={(date) => {
              if (date) {
                onChange({ start: format(date, "yyyy-MM-dd"), end })
                setStartOpen(false)
              }
            }}
            defaultMonth={startDate ?? endDate}
          />
        </PopoverContent>
      </Popover>
      <span>to</span>
      <Popover open={endOpen} onOpenChange={setEndOpen}>
        <PopoverTrigger className="hover:text-foreground hover:underline transition-colors cursor-pointer tabular-nums">
          {end ? formatDate(end) : "Any"}
        </PopoverTrigger>
        <PopoverContent className="w-auto p-0" align="start">
          <Calendar
            mode="single"
            selected={endDate}
            onSelect={(date) => {
              if (date) {
                onChange({ start, end: format(date, "yyyy-MM-dd") })
                setEndOpen(false)
              }
            }}
            defaultMonth={endDate ?? startDate}
          />
        </PopoverContent>
      </Popover>
    </div>
  )
}
