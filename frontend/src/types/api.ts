// API request and response types matching the REST API surface.
// These define the contract between frontend and backend.

import type {
  Account,
  AccountType,
  Holding,
  PortfolioSnapshot,
  Transaction,
} from "./models"

export type Granularity = "monthly" | "quarterly" | "yearly"

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

export interface BudgetRow {
  category: string
  budgeted: string // Decimal string
  actual: string // Decimal string (absolute value of spending)
  percent: number // actual / budgeted * 100
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

export interface SpendingGridRow {
  category: string
  section: string // "Income" | "Bills" | "Spending" | "Irregular" | "Transfers"
  periods: Record<string, string | null> // period key -> Decimal string, null = no data
  average: string | null
  budget: string | null
  total: string | null
}

// Re-export model types used in API responses
export type {
  Account,
  AccountType,
  Holding,
  PortfolioSnapshot,
  Transaction,
}
