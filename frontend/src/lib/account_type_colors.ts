import type { AccountType } from "@/types"

const PALETTE: Record<AccountType, { bg: string; border: string; text: string }> = {
  checking: { bg: "bg-blue-500/15", border: "border-blue-500/40", text: "text-blue-500" },
  savings: { bg: "bg-emerald-500/15", border: "border-emerald-500/40", text: "text-emerald-500" },
  emergency_fund: { bg: "bg-cyan-500/15", border: "border-cyan-500/40", text: "text-cyan-500" },
  investment: { bg: "bg-violet-500/15", border: "border-violet-500/40", text: "text-violet-500" },
  investment_isa: { bg: "bg-fuchsia-500/15", border: "border-fuchsia-500/40", text: "text-fuchsia-500" },
  credit: { bg: "bg-rose-500/15", border: "border-rose-500/40", text: "text-rose-500" },
  cash: { bg: "bg-amber-500/15", border: "border-amber-500/40", text: "text-amber-500" },
  pension: { bg: "bg-indigo-500/15", border: "border-indigo-500/40", text: "text-indigo-500" },
  property: { bg: "bg-orange-500/15", border: "border-orange-500/40", text: "text-orange-500" },
}

const FALLBACK = { bg: "bg-secondary", border: "border-border", text: "text-foreground" }

export function accountTypeClasses(type: string): string {
  const p = PALETTE[type as AccountType] ?? FALLBACK
  return `${p.bg} ${p.border} ${p.text}`
}
