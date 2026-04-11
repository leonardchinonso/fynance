import { useUrlFilters, type Preset } from "@/hooks/use_url_filters"
import type { Granularity } from "@/types"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"

const PRESETS: { value: Preset; label: string }[] = [
  { value: "this-month", label: "This month" },
  { value: "last-3-months", label: "Last 3 months" },
  { value: "last-12-months", label: "Last 12 months" },
  { value: "ytd", label: "Year to date" },
  { value: "full-year", label: "Full year" },
]

interface DateRangeSelectorProps {
  showGranularity?: boolean
}

export function DateRangeSelector({
  showGranularity = false,
}: DateRangeSelectorProps) {
  const { preset, setPreset, granularity, setGranularity, start, end } =
    useUrlFilters()

  return (
    <div className="flex flex-wrap items-center gap-3">
      <Select value={preset} onValueChange={(v) => setPreset(v as Preset)}>
        <SelectTrigger className="w-[180px]">
          <span>{PRESETS.find((p) => p.value === preset)?.label ?? preset}</span>
        </SelectTrigger>
        <SelectContent>
          {PRESETS.map((p) => (
            <SelectItem key={p.value} value={p.value}>
              {p.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>

      <span className="text-sm text-muted-foreground">
        {start} to {end}
      </span>

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
