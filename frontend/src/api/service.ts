import type {
  Account,
  AccountSnapshot,
  BudgetRow,
  CashFlowMonth,
  CategoryTotal,
  CategoryTotalFilters,
  CreateAccountBody,
  PatchAccountBody,
  CreateCategoryBody,
  Currency,
  PatchCategoryBody,
  PatchTransactionBody,
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
import type { Category } from "@/bindings/Category"
import type { CategoryNode } from "@/bindings/CategoryNode"
import type { IngestionPreview } from "@/bindings/IngestionPreview"
import type { ParseHints } from "@/bindings/ParseHints"
import type { ImportPayload } from "@/bindings/ImportPayload"
import type { HoldingsImportPayload } from "@/bindings/HoldingsImportPayload"
import type { InvestmentsImportPayload } from "@/bindings/InvestmentsImportPayload"
import type { InvestmentImportResult } from "@/bindings/InvestmentImportResult"
import type { CapitalGainsResponse } from "@/bindings/CapitalGainsResponse"

export interface HoldingsImportResponse { inserted: number; updated: number; total: number }

/**
 * Period selector for a CGT query. Discriminated so it's impossible to mix
 * conflicting date inputs from the UI. The wire format always collapses to
 * `start_date` / `end_date`:
 *  - `tax-year` resolves to 6 Apr → 5 Apr next year client-side
 *  - `range` is sent as-is
 *  - `as-at` sends only `end_date`; the backend treats absent start as "from time zero",
 *    which is the same as the engine's `as_at` semantics for the report use case.
 */
export type CgtPeriod =
  | { kind: "tax-year"; taxYear: string }
  | { kind: "range"; startDate: string; endDate: string }
  | { kind: "as-at"; asAt: string }

export interface CgtFilters {
  period: CgtPeriod
  /** Exactly one profile — the report has no "all profiles" mode. */
  profileId: string
}

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
  /** Leaf categories as `{id, name}` where name is `"Parent: Child"`. */
  getCategoriesWithIds(): Promise<Array<{ id: string; name: string }>>
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
  getHoldingsBatch(accountIds: string[]): Promise<Holding[]>
  getCashFlow(
    start: string,
    end: string,
    granularity?: Granularity,
    profileId?: string,
    excludeCategoryIds?: string[]
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
  updateProfile(id: string, body: { name?: string }): Promise<Profile>
  deleteProfile(id: string): Promise<void>

  createAccount(body: CreateAccountBody): Promise<Account>
  updateAccount(id: string, body: PatchAccountBody): Promise<Account>
  deleteAccount(id: string): Promise<void>

  getCategoryDetails(): Promise<CategoryNode[]>
  createCategory(body: CreateCategoryBody): Promise<Category>
  updateCategory(id: string, body: PatchCategoryBody): Promise<Category>
  deleteCategory(id: string): Promise<void>
  patchTransaction(id: string, body: PatchTransactionBody): Promise<Transaction>

  // ── Currencies ────────────────────────────────────────────────────
  getCurrencies(): Promise<Currency[]>
  createCurrency(body: { code: string; fx_rate: string }): Promise<Currency>
  updateCurrency(code: string, body: { fx_rate?: string; is_preferred?: boolean }): Promise<Currency>
  deleteCurrency(code: string): Promise<void>

  // ── Import ────────────────────────────────────────────────────────
  importCsv(accountId: string, file: File): Promise<ImportResult>

  /** Stage 1: upload files to `/api/parse`. Returns a preview with payloads. */
  parseDocuments(files: File[], accountId: string, hints: ParseHints): Promise<IngestionPreview>
  /** Stage 2: commit transactions via `/api/transactions/import`. */
  commitTransactions(payload: ImportPayload): Promise<ImportResult>
  /** Stage 2: commit holdings via `/api/holdings/import`. */
  commitHoldings(payload: HoldingsImportPayload): Promise<HoldingsImportResponse>
  /** Stage 2: commit investment events via `/api/investments/import`. */
  commitInvestments(payload: InvestmentsImportPayload): Promise<InvestmentImportResult>

  // ── Reports ───────────────────────────────────────────────────────
  /**
   * Fetch the UK CGT report for a period and profile set. Maps to
   * `GET /api/investments/capital-gains`. See [CgtFilters] for input shape.
   */
  getCapitalGains(filters: CgtFilters): Promise<CapitalGainsResponse>
}
