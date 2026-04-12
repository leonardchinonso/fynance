import type {
  Account,
  BudgetRow,
  BudgetUpdateRequest,
  CashFlowMonth,
  Granularity,
  Holding,
  PaginatedResponse,
  PortfolioHistoryRow,
  PortfolioResponse,
  AccountSnapshot,
  Profile,
  SpendingGridRow,
  Transaction,
  TransactionFilters,
} from "@/types"

/**
 * ApiService defines the contract between the frontend and backend.
 * Components import and use this interface exclusively.
 *
 * The mock implementation returns realistic data with a 500ms delay.
 * When the Rust backend is ready, swap MockApiService for RealApiService
 * in client.ts. Zero component changes needed.
 */
export interface ApiService {
  // Profiles
  getProfiles(): Promise<Profile[]>

  // Transactions
  getTransactions(
    filters: TransactionFilters
  ): Promise<PaginatedResponse<Transaction>>
  getCategories(): Promise<string[]>
  getAccounts(profileId?: string): Promise<Account[]>

  // Budget
  getBudget(month: string): Promise<BudgetRow[]>
  getSpendingGrid(
    start: string,
    end: string,
    granularity: Granularity,
    profileId?: string
  ): Promise<SpendingGridRow[]>
  updateBudget(req: BudgetUpdateRequest): Promise<void>

  // Portfolio
  getPortfolio(profileId?: string): Promise<PortfolioResponse>
  getPortfolioHistory(
    start?: string,
    end?: string
  ): Promise<PortfolioHistoryRow[]>
  getHoldings(accountId: string): Promise<Holding[]>
  getCashFlow(start?: string, end?: string): Promise<CashFlowMonth[]>

  // Account balances (per-account monthly balances for delta calculations)
  getAccountBalances(
    start?: string,
    end?: string
  ): Promise<AccountSnapshot[]>

  // Export
  exportData(format: string): Promise<void>
}
