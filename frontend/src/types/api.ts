// API request and response types matching the REST API surface.
// These define the contract between frontend and backend.
// Types shared with backend are imported from generated bindings.

// ── From backend bindings (single source of truth) ─────────────────
export type { BalanceDelta } from "@/bindings/BalanceDelta"
export type { BreakdownItem } from "@/bindings/BreakdownItem"
export type { BudgetRow } from "@/bindings/BudgetRow"
export type { CashFlowMonth } from "@/bindings/CashFlowMonth"
export type { CategoryTotal } from "@/bindings/CategoryTotal"
export type { Granularity } from "@/bindings/Granularity"
export type { InvestmentMetrics } from "@/bindings/InvestmentMetrics"
export type { PortfolioHistoryRow } from "@/bindings/PortfolioHistoryRow"
export type { PortfolioResponse } from "@/bindings/PortfolioResponse"
export type { SetBudgetOverrideBody } from "@/bindings/SetBudgetOverrideBody"
export type { SetStandingBudgetBody } from "@/bindings/SetStandingBudgetBody"
export type { SpendingGridRow } from "@/bindings/SpendingGridRow"
export type { TransactionDirection } from "@/bindings/TransactionDirection"

// ── Frontend-only API types ─────────────────────────────────────────

export interface TransactionFilters {
  start?: string // YYYY-MM-DD
  end?: string // YYYY-MM-DD
  accounts?: string[] // account IDs
  categories?: string[] // category strings
  search?: string // free-text search across merchant, category, account, notes
  page?: number
  limit?: number
  profile_id?: string
}

// Filters for GET /api/transactions/by-category.
// When `direction` is set, totals are non-negative (absolute values) and the
// response only includes rows where matching transactions exist for that sign.
// When omitted the totals are signed net sums per category.
export interface CategoryTotalFilters {
  start?: string // YYYY-MM-DD
  end?: string // YYYY-MM-DD
  accounts?: string[]
  categories?: string[]
  profile_id?: string
  direction?: "outflow" | "income"
}

export interface PaginatedResponse<T> {
  data: T[]
  total: number
  page: number
  limit: number
}

export interface DateRange {
  start: string // YYYY-MM-DD
  end: string // YYYY-MM-DD
}

// Re-export model types used in API responses
export type { Account, AccountSnapshot, AccountType, Holding, Transaction } from "./models"
export type { CategoryDetail, CreateAccountBody, ImportResult } from "./models"
