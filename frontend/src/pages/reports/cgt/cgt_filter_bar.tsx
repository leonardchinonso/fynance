import { useState } from "react"
import { format, parse } from "date-fns"
import { Button } from "@/components/ui/button"
import { Select, SelectContent, SelectItem, SelectTrigger } from "@/components/ui/select"
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover"
import { Calendar } from "@/components/ui/calendar"
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
  onGenerate: (filters: CgtFilters, higherRate: boolean) => void
}

export function CgtFilterBar({ profiles, initial, loading, onGenerate }: CgtFilterBarProps) {
  const [preset, setPreset] = useState<PresetValue>(initialPreset(initial.period))
  const [startDate, setStartDate] = useState(initialStart(initial.period))
  const [endDate, setEndDate] = useState(initialEnd(initial.period))
  const [profileId, setProfileId] = useState(initial.profileId)
  // Frontend-only: the rate band drives the client-side tax estimate, not the
  // backend query, so it is not part of CgtFilters / the service contract.
  const [higherRate, setHigherRate] = useState(true)

  const period: CgtPeriod = buildPeriod(preset, startDate, endDate)
  const canGenerate = !loading && profileId !== ""

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

      <Select value={profileId} onValueChange={(v) => v && setProfileId(v)}>
        <SelectTrigger className="w-[160px]">
          <span className="truncate">
            {profiles.find((p) => p.id === profileId)?.name ?? "Select profile"}
          </span>
        </SelectTrigger>
        <SelectContent>
          {profiles.map((p) => (
            <SelectItem key={p.id} value={p.id}>{p.name}</SelectItem>
          ))}
        </SelectContent>
      </Select>

      <Select
        value={higherRate ? "higher" : "basic"}
        onValueChange={(v) => setHigherRate(v === "higher")}
      >
        <SelectTrigger className="w-[190px]">
          <span className="truncate">
            {higherRate ? "Higher/additional rate" : "Basic rate"}
          </span>
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="higher">Higher/additional rate</SelectItem>
          <SelectItem value="basic">Basic rate</SelectItem>
        </SelectContent>
      </Select>

      <Button
        className="ml-auto"
        onClick={() => onGenerate({ period, profileId }, higherRate)}
        disabled={!canGenerate}
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
