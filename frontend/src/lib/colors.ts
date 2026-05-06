import type { AccountType } from "@/types"

export const ACCOUNT_TYPE_COLORS: Record<AccountType, string> = {
  checking: "#3b82f6", // blue-500
  savings: "#22c55e", // green-500
  investment: "#a855f7", // purple-500
  credit: "#ef4444", // red-500
  cash: "#eab308", // yellow-500
  pension: "#6366f1", // indigo-500
  property: "#14b8a6", // teal-500
  mortgage: "#f87171", // red-400
}

export const ACCOUNT_TYPE_LABELS: Record<AccountType, string> = {
  checking: "Checking",
  savings: "Savings",
  investment: "Investment",
  credit: "Credit",
  cash: "Cash",
  pension: "Pension",
  property: "Property",
  mortgage: "Mortgage",
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

export const BUDGET_STATUS_COLORS = {
  green: "#22c55e", // < 80%
  amber: "#f59e0b", // 80-110%
  red: "#ef4444", // > 110%
} as const

export function getBudgetStatusColor(percent: number): string {
  if (percent > 110) return BUDGET_STATUS_COLORS.red
  if (percent >= 80) return BUDGET_STATUS_COLORS.amber
  return BUDGET_STATUS_COLORS.green
}

export function getBudgetStatusClass(percent: number): string {
  if (percent > 110) return "text-red-500"
  if (percent >= 80) return "text-amber-500"
  return "text-green-500"
}

export function getBudgetProgressClass(percent: number): string {
  if (percent > 110) return "[&>div]:bg-red-500"
  if (percent >= 80) return "[&>div]:bg-amber-500"
  return "[&>div]:bg-green-500"
}
