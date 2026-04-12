// API request and response types matching the REST API surface.
// These define the contract between frontend and backend.
// Types shared with backend are imported from generated bindings.

import type { Account } from "./models"

// ── From backend bindings ───────────────────────────────────────────
export type { BudgetRow } from "@/bindings/BudgetRow"
export type { Granularity } from "@/bindings/Granularity"
export type { SpendingGridRow } from "@/bindings/SpendingGridRow"

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

export interface PaginatedResponse<T> {
  data: T[]
  total: number
  page: number
  limit: number
}

export interface BudgetUpdateRequest {
  month: string // YYYY-MM
  category: string
  amount: string // Decimal string
}

export interface PortfolioBreakdownItem {
  label: string // account type or institution name
  total: string // Decimal string
  percent: number
}

export interface PortfolioResponse {
  net_worth: string
  currency: string
  as_of: string // YYYY-MM-DD
  total_assets: string
  total_liabilities: string
  available_wealth: string // checking + savings + investment
  unavailable_wealth: string // pension + home equity
  accounts: Account[]
  by_type: PortfolioBreakdownItem[]
  by_institution: PortfolioBreakdownItem[]
  by_sector: PortfolioBreakdownItem[]
}

export interface PortfolioHistoryRow {
  month: string // YYYY-MM
  available_wealth: string
  unavailable_wealth: string
  total_wealth: string
}

export interface DateRange {
  start: string // YYYY-MM-DD
  end: string // YYYY-MM-DD
}

export interface CashFlowMonth {
  month: string // YYYY-MM
  income: string // Decimal string
  spending: string // Decimal string
}

// Re-export model types used in API responses
export type { Account, AccountType, Holding, PortfolioSnapshot, Transaction } from "./models"
