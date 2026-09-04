import type { CgtFilters, CgtPeriod } from "./service"

/** Convert UI `CgtFilters` into the wire-format query params for `/api/investments/capital-gains`. */
export function cgtFiltersToParams(filters: CgtFilters): Record<string, string> {
  const params: Record<string, string> = { profile_ids: filters.profileId }
  Object.assign(params, periodToParams(filters.period))
  // Ask for the tax computation only when the period is a whole UK tax year.
  // A custom range or an as-at date has no tax year to compute against, and
  // deriving one from the dates would silently apply a year's allowance to a
  // window that is not that year.
  if (filters.period.kind === "tax-year") {
    params.tax_year = filters.period.taxYear
  }
  return params
}

function periodToParams(period: CgtPeriod): Record<string, string> {
  switch (period.kind) {
    case "tax-year": {
      const [start, end] = ukTaxYearToDates(period.taxYear)
      return { start_date: start, end_date: end }
    }
    case "range":
      return { start_date: period.startDate, end_date: period.endDate }
    case "as-at":
      return { end_date: period.asAt }
  }
}

/** Accepts `"2024-25"` or `"2024-2025"`. Returns `["YYYY-04-06", "YYYY-04-05"]`. */
export function ukTaxYearToDates(taxYear: string): [string, string] {
  const parts = taxYear.split("-")
  if (parts.length !== 2) throw new Error(`invalid tax year: ${taxYear}`)
  const startYear = Number.parseInt(parts[0], 10)
  if (!Number.isFinite(startYear)) throw new Error(`invalid tax year: ${taxYear}`)
  const endSuffix = parts[1]
  const endYear =
    endSuffix.length === 2
      ? Math.floor(startYear / 100) * 100 + Number.parseInt(endSuffix, 10)
      : Number.parseInt(endSuffix, 10)
  if (!Number.isFinite(endYear) || endYear !== startYear + 1) {
    throw new Error(`invalid tax year: ${taxYear}`)
  }
  return [`${startYear}-04-06`, `${endYear}-04-05`]
}

/** Returns the UK tax year string (e.g. `"2024-25"`) that contains `date`. */
export function ukTaxYearForDate(date: Date): string {
  const year = date.getFullYear()
  const month = date.getMonth() + 1
  const day = date.getDate()
  const startYear = month > 4 || (month === 4 && day >= 6) ? year : year - 1
  const endShort = String((startYear + 1) % 100).padStart(2, "0")
  return `${startYear}-${endShort}`
}

/** Returns the **previous** UK tax year string relative to `date`. */
export function previousUkTaxYearForDate(date: Date): string {
  const current = ukTaxYearForDate(date)
  const startYear = Number.parseInt(current.slice(0, 4), 10)
  const prevStart = startYear - 1
  const prevEndShort = String(startYear % 100).padStart(2, "0")
  return `${prevStart}-${prevEndShort}`
}

/** Human-readable label for the period, used in history rows + filenames. */
export function periodLabel(period: CgtPeriod): string {
  switch (period.kind) {
    case "tax-year":
      return `Tax year ${period.taxYear}`
    case "range":
      return `${period.startDate} – ${period.endDate}`
    case "as-at":
      return `As at ${period.asAt}`
  }
}

/** Filename-safe slug for the period. */
export function periodSlug(period: CgtPeriod): string {
  switch (period.kind) {
    case "tax-year":
      return `tax-${period.taxYear}`
    case "range":
      return `${period.startDate}_to_${period.endDate}`
    case "as-at":
      return `as-at-${period.asAt}`
  }
}
