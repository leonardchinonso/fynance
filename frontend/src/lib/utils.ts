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

// Subscribers notified when the flag flips. Components that format money during
// render subscribe via useRedactedFlag() so a toggle re-renders them in place
// (no remount, so page state like dialogs, selections, and wizard edits survives).
const redactedListeners = new Set<() => void>()

export function getRedacted(): boolean {
  return _redacted
}

export function subscribeRedacted(cb: () => void): () => void {
  redactedListeners.add(cb)
  return () => {
    redactedListeners.delete(cb)
  }
}

export function setRedacted(value: boolean): void {
  _redacted = value
  try {
    localStorage.setItem(REDACTED_KEY, value ? "1" : "0")
  } catch {
    /* ignore: redaction still works in-memory for this session */
  }
  for (const cb of redactedListeners) cb()
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

/**
 * Compact currency for tight spaces (chart axes, legends): abbreviates large
 * magnitudes to k / m / b so values never overflow and get squashed, e.g.
 * £120k, £1.4m, -£950, £0. Use the full `formatCurrency` for tables/tooltips.
 */
export function formatCurrencyCompact(amount: string | number, currency: string = "GBP"): string {
  const num = typeof amount === "number" ? amount : parseFloat(amount)
  if (amount === "" || amount == null || isNaN(num)) return "-"
  const symbol = CURRENCY_SYMBOLS[currency] ?? currency + " "
  const abs = Math.abs(num)
  const fmt = (n: number) => n.toLocaleString("en-GB", { maximumFractionDigits: 1 })
  let body: string
  if (abs >= 1e9) body = fmt(abs / 1e9) + "b"
  else if (abs >= 1e6) body = fmt(abs / 1e6) + "m"
  else if (abs >= 1e3) body = fmt(abs / 1e3) + "k"
  else body = fmt(abs)
  const result = (num < 0 ? "-" : "") + symbol + body
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
 * Format a period key for display.
 * Monthly: "Oct 25", Quarterly: "Q1 2024", Yearly: "2024"
 */
export function formatPeriodKey(
  key: string,
  granularity: "monthly" | "quarterly" | "yearly"
): string {
  if (granularity === "monthly") return formatMonthShort(key)
  // Backend quarterly keys are "YYYY-Qn"; render as "Qn YYYY". Yearly "YYYY"
  // (and any already-readable label) passes through.
  const q = key.match(/^(\d{4})-Q(\d)$/)
  return q ? `Q${q[2]} ${q[1]}` : key
}

/**
 * The backend period key a "YYYY-MM" month belongs to, matching the keys the
 * spending grid returns in `SpendingGridRow.periods`: "YYYY-MM" (monthly),
 * "YYYY-Qn" (quarterly), or "YYYY" (yearly). Used to map range months onto the
 * backend's pre-bucketed periods (e.g. to scale a monthly budget per period).
 */
/**
 * Ordered union of every period key present across spending-grid rows. The
 * backend returns each row sparsely (only periods that category has data for),
 * so taking one row's keys both drops columns and yields `undefined` lookups
 * (→ NaN) for the rest. The union + chronological sort gives the full column set;
 * lexicographic sort is chronological for "YYYY-MM" / "YYYY-Qn" / "YYYY".
 */
export function periodKeysFromRows(rows: { periods: Record<string, unknown> }[]): string[] {
  const keys = new Set<string>()
  for (const r of rows) for (const k of Object.keys(r.periods)) keys.add(k)
  return Array.from(keys).sort()
}

export function periodKeyForMonth(
  month: string,
  granularity: "monthly" | "quarterly" | "yearly"
): string {
  if (granularity === "yearly") return month.slice(0, 4)
  if (granularity === "quarterly") {
    const [y, m] = month.split("-").map(Number)
    return `${y}-Q${Math.ceil(m / 3)}`
  }
  return month
}

/**
 * The inclusive [start, end] date range (YYYY-MM-DD) a backend period key spans:
 * "YYYY-MM" → that calendar month, "YYYY-Qn" → that quarter, "YYYY" → that year.
 * Used to drill a clicked chart period down to the matching transactions.
 */
export function periodKeyToRange(
  key: string,
  granularity: "monthly" | "quarterly" | "yearly"
): { start: string; end: string } {
  const pad = (n: number) => String(n).padStart(2, "0")
  const lastDay = (year: number, month1: number) => new Date(year, month1, 0).getDate()
  if (granularity === "yearly") {
    return { start: `${key}-01-01`, end: `${key}-12-31` }
  }
  if (granularity === "quarterly") {
    const m = key.match(/^(\d{4})-Q(\d)$/)
    if (m) {
      const year = Number(m[1])
      const quarter = Number(m[2])
      const startMonth = (quarter - 1) * 3 + 1
      const endMonth = quarter * 3
      return { start: `${m[1]}-${pad(startMonth)}-01`, end: `${m[1]}-${pad(endMonth)}-${pad(lastDay(year, endMonth))}` }
    }
  }
  // monthly "YYYY-MM"
  const [y, mo] = key.split("-")
  return { start: `${y}-${mo}-01`, end: `${y}-${mo}-${pad(lastDay(Number(y), Number(mo)))}` }
}
