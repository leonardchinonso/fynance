import { useState } from "react"
import { format, parse } from "date-fns"
import { Check, ChevronsUpDown } from "lucide-react"
import { cn } from "@/lib/utils"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { Select, SelectContent, SelectItem, SelectTrigger } from "@/components/ui/select"
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover"
import { Calendar } from "@/components/ui/calendar"
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command"
import type { Profile } from "@/types"
import type { CgtFilters, CgtPeriod } from "@/api/service"
import { ukTaxYearToDates } from "@/api/cgt_filter_params"

const TAX_YEAR_PRESETS = ["2025-26", "2024-25", "2023-24", "2022-23", "2021-22"] as const
type PresetValue =
  | (typeof TAX_YEAR_PRESETS)[number]
  | "custom-range"
  | "as-at"

interface CgtFilterBarProps {
  profiles: Profile[]
  initial: CgtFilters
  loading: boolean
  onGenerate: (filters: CgtFilters) => void
}

export function CgtFilterBar({ profiles, initial, loading, onGenerate }: CgtFilterBarProps) {
  const [preset, setPreset] = useState<PresetValue>(initialPreset(initial.period))
  const [startDate, setStartDate] = useState(initialStart(initial.period))
  const [endDate, setEndDate] = useState(initialEnd(initial.period))
  const [profileIds, setProfileIds] = useState<string[]>(initial.profileIds)

  const period: CgtPeriod = buildPeriod(preset, startDate, endDate)

  return (
    <div className="flex flex-wrap items-center gap-2 sm:gap-3">
      <Select value={preset} onValueChange={(v) => onPresetChange(v as PresetValue)}>
        <SelectTrigger className="w-[200px]">
          <span>{labelFor(preset)}</span>
        </SelectTrigger>
        <SelectContent>
          {TAX_YEAR_PRESETS.map((ty) => (
            <SelectItem key={ty} value={ty}>{`${ty} Tax Year`}</SelectItem>
          ))}
          <div className="my-1 border-t border-border" />
          <SelectItem value="custom-range">Custom range</SelectItem>
          <SelectItem value="as-at">As at a date</SelectItem>
        </SelectContent>
      </Select>

      {(preset === "custom-range" || preset === "as-at") && (
        <div className="flex items-center gap-1 text-sm text-muted-foreground">
          {preset === "custom-range" && (
            <>
              <DatePicker value={startDate} onChange={setStartDate} />
              <span>to</span>
            </>
          )}
          <DatePicker value={endDate} onChange={setEndDate} />
        </div>
      )}

      <ProfileMultiSelect
        profiles={profiles}
        selected={profileIds}
        onChange={setProfileIds}
      />

      <Button
        className="ml-auto"
        onClick={() => onGenerate({ period, profileIds })}
        disabled={loading}
      >
        {loading ? "Generating…" : "Generate"}
      </Button>
    </div>
  )

  function onPresetChange(next: PresetValue) {
    setPreset(next)
    if (next !== "custom-range" && next !== "as-at") {
      const [s, e] = ukTaxYearToDates(next)
      setStartDate(s)
      setEndDate(e)
    }
  }
}

function DatePicker({ value, onChange }: { value: string; onChange: (s: string) => void }) {
  const [open, setOpen] = useState(false)
  const date = parse(value, "yyyy-MM-dd", new Date())
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger className="hover:text-foreground hover:underline transition-colors cursor-pointer">
        {value}
      </PopoverTrigger>
      <PopoverContent className="w-auto p-0" align="start">
        <Calendar
          mode="single"
          selected={date}
          onSelect={(d) => {
            if (d) {
              onChange(format(d, "yyyy-MM-dd"))
              setOpen(false)
            }
          }}
          defaultMonth={date}
        />
      </PopoverContent>
    </Popover>
  )
}

function ProfileMultiSelect({
  profiles,
  selected,
  onChange,
}: {
  profiles: Profile[]
  selected: string[]
  onChange: (ids: string[]) => void
}) {
  const [open, setOpen] = useState(false)
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger className="inline-flex shrink-0 items-center justify-center gap-1 rounded-md border bg-background px-3 py-1 text-sm font-medium shadow-xs hover:bg-accent hover:text-accent-foreground h-9">
        Profiles
        {selected.length > 0 && (
          <Badge variant="secondary" className="ml-1">
            {selected.length}
          </Badge>
        )}
        <ChevronsUpDown className="ml-1 h-3 w-3 opacity-50" />
      </PopoverTrigger>
      <PopoverContent className="w-[250px] p-0" align="start">
        <Command>
          <CommandInput placeholder="Search profiles…" />
          <CommandList>
            <CommandEmpty>No profiles.</CommandEmpty>
            <CommandGroup>
              {profiles.map((p) => (
                <CommandItem
                  key={p.id}
                  onSelect={() =>
                    onChange(
                      selected.includes(p.id)
                        ? selected.filter((id) => id !== p.id)
                        : [...selected, p.id],
                    )
                  }
                >
                  <Check
                    className={cn(
                      "mr-2 h-4 w-4",
                      selected.includes(p.id) ? "opacity-100" : "opacity-0",
                    )}
                  />
                  {p.name}
                </CommandItem>
              ))}
            </CommandGroup>
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  )
}

function buildPeriod(preset: PresetValue, startDate: string, endDate: string): CgtPeriod {
  if (preset === "custom-range") return { kind: "range", startDate, endDate }
  if (preset === "as-at") return { kind: "as-at", asAt: endDate }
  return { kind: "tax-year", taxYear: preset }
}

function initialPreset(period: CgtPeriod): PresetValue {
  if (period.kind === "tax-year") {
    const known = TAX_YEAR_PRESETS.find((p) => p === period.taxYear)
    return (known ?? "custom-range") as PresetValue
  }
  if (period.kind === "range") return "custom-range"
  return "as-at"
}

function initialStart(period: CgtPeriod): string {
  if (period.kind === "range") return period.startDate
  if (period.kind === "tax-year") return ukTaxYearToDates(period.taxYear)[0]
  return ""
}

function initialEnd(period: CgtPeriod): string {
  if (period.kind === "range") return period.endDate
  if (period.kind === "tax-year") return ukTaxYearToDates(period.taxYear)[1]
  return period.asAt
}

function labelFor(preset: PresetValue): string {
  if (preset === "custom-range") return "Custom range"
  if (preset === "as-at") return "As at a date"
  return `${preset} Tax Year`
}
