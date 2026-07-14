import type { AccountType } from "@/types"
import type { InvestmentEventType } from "@/bindings/InvestmentEventType"

export const ACCOUNT_TYPE_COLORS: Record<AccountType, string> = {
  checking: "#3b82f6",     // blue-500
  savings: "#22c55e",      // green-500
  emergency_fund: "#06b6d4", // cyan-500
  investment: "#a855f7",   // purple-500
  investment_isa: "#c084fc", // purple-400
  credit: "#ef4444",       // red-500
  pension: "#6366f1",      // indigo-500
  property: "#14b8a6",     // teal-500
}

export const ACCOUNT_TYPE_LABELS: Record<AccountType, string> = {
  checking: "Checking",
  savings: "Savings",
  emergency_fund: "Emergency fund",
  investment: "Investment",
  investment_isa: "Investment ISA",
  credit: "Credit",
  pension: "Pension",
  property: "Property",
}

// Approved palette — used for auto-assigning colors to new parent categories
export const COLOR_PALETTE = [
  "#22c55e", // green-500
  "#3b82f6", // blue-500
  "#f97316", // orange-500
  "#06b6d4", // cyan-500
  "#ec4899", // pink-500
  "#a855f7", // purple-500
  "#eab308", // yellow-500
  "#14b8a6", // teal-500
  "#6366f1", // indigo-500
  "#f43f5e", // rose-500
  "#d946ef", // fuchsia-500
  "#0ea5e9", // sky-500
  "#78716c", // stone-500
  "#84cc16", // lime-500
  "#f59e0b", // amber-500
  "#10b981", // emerald-500
]

// Stable seed colors for well-known parent categories
export const CATEGORY_COLORS: Record<string, string> = {
  Income: "#22c55e",
  Housing: "#3b82f6",
  Food: "#f97316",
  Transport: "#06b6d4",
  Health: "#ec4899",
  Shopping: "#a855f7",
  Entertainment: "#eab308",
  Travel: "#14b8a6",
  Finance: "#6366f1",
  "Personal Care": "#f43f5e",
  "Gifts & Donations": "#d946ef",
  Education: "#0ea5e9",
  Other: "#78716c",
}

// Palette for ticker symbols. Distinct hues so adjacent symbols stay legible.
export const STOCK_COLORS = [
  "#3b82f6", "#f97316", "#22c55e", "#a855f7", "#ec4899",
  "#06b6d4", "#eab308", "#6366f1", "#14b8a6", "#ef4444",
]

/**
 * Stable color for a ticker. Hashed from the symbol rather than assigned by
 * position, so a symbol keeps the same color across every chart and table and
 * doesn't shift when the surrounding set is reordered or filtered.
 */
export function colorForSymbol(symbol: string): string {
  let hash = 0
  for (let i = 0; i < symbol.length; i++) {
    hash = (hash * 31 + symbol.charCodeAt(i)) | 0
  }
  return STOCK_COLORS[Math.abs(hash) % STOCK_COLORS.length]
}

/**
 * Investment event types, colored by what they do to a position: acquisitions
 * green, disposals red, and neutral movements blue.
 */
export const EVENT_TYPE_COLORS: Record<InvestmentEventType, string> = {
  buy: "#22c55e",       // green-500   — acquisition
  vest: "#10b981",      // emerald-500 — acquisition (granted)
  transfer: "#3b82f6",  // blue-500    — neutral movement
  sell: "#ef4444",      // red-500     — disposal
  withhold: "#f97316",  // orange-500  — disposal (tax)
  split: "#6366f1",     // indigo-500  — neutral restructure
}
