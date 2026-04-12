// Domain types: re-exported from backend-generated bindings (ts-rs).
// Types still used only by mock data keep local definitions until
// their backend endpoints are wired up.

// ── From backend bindings (single source of truth) ──────────────────
export type { Account } from "@/bindings/Account"
export type { AccountType } from "@/bindings/AccountType"
export type { CategorySource } from "@/bindings/CategorySource"
export type { Profile } from "@/bindings/Profile"
export type { Transaction } from "@/bindings/Transaction"
export type { PortfolioSnapshot } from "@/bindings/PortfolioSnapshot"
export type { HoldingType } from "@/bindings/HoldingType"
export type { Holding } from "@/bindings/Holding"

// ── Local types (not yet in backend or differ for mock usage) ───────

export type IngestionStatus = "pending" | "completed" | "skipped"

export interface Budget {
  month: string // YYYY-MM
  category: string
  amount: string // Decimal string, monthly budget cap
}

export interface IngestionChecklistItem {
  month: string // YYYY-MM
  account_id: string
  status: IngestionStatus
  completed_at: string | null
  notes: string | null
}
