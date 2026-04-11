import { clsx, type ClassValue } from "clsx"
import { twMerge } from "tailwind-merge"
import {
  format,
  parse,
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

export function formatCurrency(amount: string, currency: string = "GBP"): string {
  const num = parseFloat(amount)
  const symbol = CURRENCY_SYMBOLS[currency] ?? currency + " "
  const abs = Math.abs(num)
  const formatted =
    symbol +
    abs.toLocaleString("en-GB", {
      minimumFractionDigits: 2,
      maximumFractionDigits: 2,
    })
  return num < 0 ? `-${formatted}` : formatted
}

export function formatDate(dateStr: string): string {
  const date = parse(dateStr, "yyyy-MM-dd", new Date())
  return format(date, "dd MMM yyyy")
}

export function formatMonth(month: string): string {
  const date = parse(month + "-01", "yyyy-MM-dd", new Date())
  return format(date, "MMM yyyy")
}

export function formatMonthShort(month: string): string {
  const date = parse(month + "-01", "yyyy-MM-dd", new Date())
  return format(date, "MMM yy")
}

export function daysSince(dateStr: string): number {
  const date = parse(dateStr, "yyyy-MM-dd", new Date())
  return differenceInDays(new Date(), date)
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
