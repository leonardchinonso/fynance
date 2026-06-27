import { clsx, type ClassValue } from "clsx"
import { twMerge } from "tailwind-merge"
import {
  format,
  parse,
  parseISO,
  startOfMonth,
  endOfMonth,
  eachMonthOfInterval,
  differenceInDays,
} from "date-fns"
import type { DisplayCurrency } from "@/bindings/DisplayCurrency"

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}

const CURRENCY_SYMBOLS: Record<string, string> = {
  GBP: "\u00a3",
  NGN: "\u20a6",
  USD: "$",
  EUR: "\u20ac",
}

/**
 * Redacted ("privacy") mode. When on, every digit in a formatted money
 * string is replaced with `*` while the currency symbol, sign, thousands
 * separators and decimal point are kept, so the shape of the value stays
 * visible but the amount does not (\u00a357,943.39 -> \u00a3\u2022\u2022,\u2022\u2022\u2022.\u2022\u2022). The bullet
 * glyph is used (not `*`) because it reads as a masked value and stays
 * visually distinct from the `.`/`,` separators we keep.
 *
 * formatCurrency is a pure module function called from many components that
 * do not consume React context, so the flag lives at module scope (mirrored
 * to localStorage for persistence). RedactedProvider owns the React state and
 * forces a re-render of money-displaying components when this is toggled.
 */
const REDACTED_KEY = "fynance:redacted"
let _redacted = (() => {
  try {
    return localStorage.getItem(REDACTED_KEY) === "1"
  } catch {
    return false
  }
})()

export function isRedactedInitial(): boolean {
  return _redacted
}

export function setRedacted(value: boolean): void {
  _redacted = value
  try {
    localStorage.setItem(REDACTED_KEY, value ? "1" : "0")
  } catch {
    /* ignore: redaction still works in-memory for this session */
  }
}

export function formatCurrency(amount: string, currency: string = "GBP"): string {
  const num = parseFloat(amount)
  if (!amount || isNaN(num)) return "-"
  const symbol = CURRENCY_SYMBOLS[currency] ?? currency + " "
  const abs = Math.abs(num)
  const formatted =
    symbol +
    abs.toLocaleString("en-GB", {
      minimumFractionDigits: 2,
      maximumFractionDigits: 2,
    })
  const result = num < 0 ? `-${formatted}` : formatted
  return _redacted ? result.replace(/\d/g, "•") : result
}

export function categoryLeaf(category: string): string {
  return category.split(": ").pop() ?? category
}

export function categoryParent(category: string): string {
  return category.split(":")[0].trim()
}

export function formatDate(dateStr: string): string {
  return format(parseISO(dateStr), "dd MMM yyyy")
}

export function formatMonth(month: string): string {
  const date = parse(month + "-01", "yyyy-MM-dd", new Date())
  // A non-"YYYY-MM" label (e.g. a quarterly "YYYY-Qn") parses to Invalid Date;
  // returning it unchanged is harmless and never throws in date-fns `format`.
  if (Number.isNaN(date.getTime())) return month
  return format(date, "MMM yyyy")
}

export function formatMonthShort(month: string): string {
  const date = parse(month + "-01", "yyyy-MM-dd", new Date())
  if (Number.isNaN(date.getTime())) return month
  return format(date, "MMM yy")
}

export function daysSince(dateStr: string): number {
  return differenceInDays(new Date(), parseISO(dateStr))
}

export function getMonthsInRange(start: string, end: string): string[] {
  const startDate = startOfMonth(parse(start, "yyyy-MM-dd", new Date()))
  const endDate = endOfMonth(parse(end, "yyyy-MM-dd", new Date()))
  return eachMonthOfInterval({ start: startDate, end: endDate }).map((d) =>
    format(d, "yyyy-MM")
  )
}

export function getMonthFromDate(date: string): string {
  return date.substring(0, 7) // YYYY-MM from YYYY-MM-DD
}

export function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

/**
 * Get the quarter label for a YYYY-MM string.
 * "2024-01" -> "Q1 2024", "2024-04" -> "Q2 2024", etc.
 */
export function getQuarter(month: string): string {
  const [y, m] = month.split("-").map(Number)
  const q = Math.ceil(m / 3)
  return `Q${q} ${y}`
}

/**
 * Get the year label for a YYYY-MM string.
 */
export function getYear(month: string): string {
  return month.substring(0, 4)
}

/**
 * Group an array of months into period keys based on granularity.
 * Returns an ordered array of unique period keys.
 */
export function groupMonthsByGranularity(
  months: string[],
  granularity: "monthly" | "quarterly" | "yearly"
): string[] {
  const keyFn =
    granularity === "quarterly"
      ? getQuarter
      : granularity === "yearly"
        ? getYear
        : (m: string) => m

  const seen = new Set<string>()
  const result: string[] = []
  for (const m of months) {
    const key = keyFn(m)
    if (!seen.has(key)) {
      seen.add(key)
      result.push(key)
    }
  }
  return result
}

/**
 * Get which months belong to a given period key.
 */
export function getMonthsForPeriod(
  allMonths: string[],
  periodKey: string,
  granularity: "monthly" | "quarterly" | "yearly"
): string[] {
  const keyFn =
    granularity === "quarterly"
      ? getQuarter
      : granularity === "yearly"
        ? getYear
        : (m: string) => m

  return allMonths.filter((m) => keyFn(m) === periodKey)
}

/**
 * Format a period key for display.
 * Monthly: "Oct 25", Quarterly: "Q1 2024", Yearly: "2024"
 */
export function formatPeriodKey(
  key: string,
  granularity: "monthly" | "quarterly" | "yearly"
): string {
  if (granularity === "monthly") return formatMonthShort(key)
  return key // Q1 2024 or 2024 are already readable
}

/**
 * Format a monetary value for display.
 * Uses display_currency when present (foreign-currency single-source value),
 * otherwise falls back to the preferred currency.
 */
export function formatMonetary(
  value: string,
  preferredCurrency: string,
  display?: DisplayCurrency | null
): string {
  if (display) return formatCurrency(display.value, display.currency)
  return formatCurrency(value, preferredCurrency)
}

/**
 * Returns a primary label and an optional secondary label (for tooltips).
 * When display_currency is present, primary shows the foreign value and
 * secondary shows the preferred-currency equivalent.
 */
export function formatMonetaryWithFallback(
  value: string,
  preferredCurrency: string,
  display?: DisplayCurrency | null
): { primary: string; secondary?: string } {
  if (display) {
    return {
      primary: formatCurrency(display.value, display.currency),
      secondary: formatCurrency(value, preferredCurrency),
    }
  }
  return { primary: formatCurrency(value, preferredCurrency) }
}
