import { useState } from "react"
import { useUrlFilters, type Preset } from "@/hooks/use_url_filters"
import type { Granularity } from "@/types"
import { formatDate } from "@/lib/utils"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
} from "@/components/ui/select"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover"
import { Calendar } from "@/components/ui/calendar"
import { parse, format } from "date-fns"

const PRESETS: { value: Preset; label: string }[] = [
  { value: "this-month", label: "This month" },
  { value: "last-3-months", label: "Last 3 months" },
  { value: "last-12-months", label: "Last 12 months" },
  { value: "ytd", label: "Year to date" },
  { value: "3-years", label: "3 years" },
  { value: "5-years", label: "5 years" },
  { value: "10-years", label: "10 years" },
]

interface DateRangeSelectorProps {
  showGranularity?: boolean
}

export function DateRangeSelector({
  showGranularity = false,
}: DateRangeSelectorProps) {
  const { preset, setPreset, granularity, setGranularity, start, end, setFilter } =
    useUrlFilters()

  const [startPickerOpen, setStartPickerOpen] = useState(false)
  const [endPickerOpen, setEndPickerOpen] = useState(false)

  function handleStartDateChange(date: Date | undefined) {
    if (date) {
      setFilter({ start: format(date, "yyyy-MM-dd"), preset: "custom" })
      setStartPickerOpen(false)
    }
  }

  function handleEndDateChange(date: Date | undefined) {
    if (date) {
      setFilter({ end: format(date, "yyyy-MM-dd"), preset: "custom" })
      setEndPickerOpen(false)
    }
  }

  const startDate = parse(start, "yyyy-MM-dd", new Date())
  const endDate = parse(end, "yyyy-MM-dd", new Date())

  return (
    <div className="flex flex-wrap items-center gap-3">
      <Select value={preset} onValueChange={(v) => setPreset(v as Preset)}>
        <SelectTrigger className="w-[160px]">
          <span>{PRESETS.find((p) => p.value === preset)?.label ?? "Custom"}</span>
        </SelectTrigger>
        <SelectContent>
          {PRESETS.map((p) => (
            <SelectItem key={p.value} value={p.value}>
              {p.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>

      {/* Clickable date range for custom selection */}
      <div className="flex items-center gap-1 text-sm text-muted-foreground">
        <Popover open={startPickerOpen} onOpenChange={setStartPickerOpen}>
          <PopoverTrigger className="hover:text-foreground hover:underline transition-colors cursor-pointer">
            {formatDate(start)}
          </PopoverTrigger>
          <PopoverContent className="w-auto p-0" align="start">
            <Calendar
              mode="single"
              selected={startDate}
              onSelect={handleStartDateChange}
              defaultMonth={startDate}
            />
          </PopoverContent>
        </Popover>
        <span>to</span>
        <Popover open={endPickerOpen} onOpenChange={setEndPickerOpen}>
          <PopoverTrigger className="hover:text-foreground hover:underline transition-colors cursor-pointer">
            {formatDate(end)}
          </PopoverTrigger>
          <PopoverContent className="w-auto p-0" align="start">
            <Calendar
              mode="single"
              selected={endDate}
              onSelect={handleEndDateChange}
              defaultMonth={endDate}
            />
          </PopoverContent>
        </Popover>
      </div>

      {showGranularity && (
        <ToggleGroup
          type="single"
          value={granularity}
          onValueChange={(v) => {
            if (v) setGranularity(v as Granularity)
          }}
          className="ml-auto"
        >
          <ToggleGroupItem value="monthly" size="sm">
            Monthly
          </ToggleGroupItem>
          <ToggleGroupItem value="quarterly" size="sm">
            Quarterly
          </ToggleGroupItem>
          <ToggleGroupItem value="yearly" size="sm">
            Yearly
          </ToggleGroupItem>
        </ToggleGroup>
      )}
    </div>
  )
}
