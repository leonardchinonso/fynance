import type {
  Account,
  AccountSnapshot,
  BudgetRow,
  CashFlowMonth,
  CategoryDetail,
  CategoryTotal,
  CategoryTotalFilters,
  CreateAccountBody,
  Granularity,
  Holding,
  ImportResult,
  PaginatedResponse,
  PortfolioHistoryRow,
  PortfolioResponse,
  Profile,
  SetBudgetOverrideBody,
  SetStandingBudgetBody,
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
  /**
   * Server-side aggregation of transactions grouped by category.
   *
   * When `filters.direction` is set, totals are absolute sums and only
   * transactions with the matching sign are included. When omitted, totals
   * are signed net sums (negative = net spend).
   *
   * Prefer this over `getTransactions` when you only need per-category
   * totals (bar/pie charts, "total spent on X") instead of raw rows.
   */
  getTransactionsByCategory(
    filters: CategoryTotalFilters
  ): Promise<CategoryTotal[]>
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
  /**
   * Set the standing monthly budget for one category. Applies to every
   * month unless a per-month override is set via `setBudgetOverride`.
   * Maps to `POST /api/budget`.
   */
  setStandingBudget(body: SetStandingBudgetBody): Promise<void>
  /**
   * Set a per-month override on top of the standing budget for one
   * category. Maps to `POST /api/budget/override`.
   */
  setBudgetOverride(body: SetBudgetOverrideBody): Promise<void>

  // Portfolio
  getPortfolio(profileId?: string): Promise<PortfolioResponse>
  getPortfolioHistory(
    start: string,
    end: string,
    granularity?: Granularity,
    profileId?: string
  ): Promise<PortfolioHistoryRow[]>
  getHoldings(accountId: string): Promise<Holding[]>
  getCashFlow(
    start: string,
    end: string,
    granularity?: Granularity,
    profileId?: string
  ): Promise<CashFlowMonth[]>

  // Account balances (per-account monthly balances for delta calculations)
  getAccountBalances(
    start: string,
    end: string,
    profileId?: string
  ): Promise<AccountSnapshot[]>

  // Export
  exportData(format: string): Promise<void>

  // ── Settings / CRUD ───────────────────────────────────────────────
  createProfile(body: { id: string; name: string }): Promise<Profile>
  createAccount(body: CreateAccountBody): Promise<Account>

  // Categories (mock-only until BE adds category CRUD endpoints)
  getCategoryDetails(): Promise<CategoryDetail[]>
  createCategory(body: { name: string; description: string; group: string }): Promise<CategoryDetail>
  updateCategory(id: string, body: { name?: string; description?: string; group?: string }): Promise<CategoryDetail>
  deleteCategory(id: string): Promise<void>

  // ── Import ────────────────────────────────────────────────────────
  importCsv(accountId: string, file: File): Promise<ImportResult>
}
