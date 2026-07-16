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
  Paginated,
  PortfolioHistoryRow,
  PortfolioResponse,
  Profile,
  SetBudgetOverrideBody,
  SetStandingBudgetBody,
  SpendingGridRow,
  SpendingGridFilters,
  CashSummaryResponse,
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
import type { InvestmentEvent } from "@/bindings/InvestmentEvent"
import type { InvestmentHistoryRow } from "@/bindings/InvestmentHistoryRow"
import type { CreateInvestmentEventBody } from "@/bindings/CreateInvestmentEventBody"
import type { PatchInvestmentEventBody } from "@/bindings/PatchInvestmentEventBody"
import type { S104PoolState } from "@/bindings/S104PoolState"
import type { CapitalGainsResponse } from "@/bindings/CapitalGainsResponse"
import type { DocumentSummary } from "@/bindings/DocumentSummary"
import type { DocumentDeleteResult } from "@/bindings/DocumentDeleteResult"
import type { AccountHoldingSeries } from "@/bindings/AccountHoldingSeries"
import type { AccountHoldingHistoryRow } from "@/bindings/AccountHoldingHistoryRow"
// Aliased: the generated binding's `ProgressEvent` shadows the DOM global.
import type { ProgressEvent as ParseProgressEvent } from "@/bindings/ProgressEvent"

/** Response of `GET /api/holdings/account-history`: per-holding value series for one account. */
export interface AccountHoldingsHistory {
  preferred_currency: string
  symbols: AccountHoldingSeries[]
  rows: AccountHoldingHistoryRow[]
}

export type { ParseProgressEvent }
export type ParseProgressHandler = (event: ParseProgressEvent) => void
/** Optional streaming-progress wiring for `parseDocuments`. */
export interface ParseOptions {
  /** Client-generated id correlating the upload with its SSE progress stream. */
  parseId?: string
  /** Called for every progress event received on the SSE stream. */
  onProgress?: ParseProgressHandler
}

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
  ): Promise<Paginated<Transaction>>
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
    profileId?: string,
    filters?: SpendingGridFilters
  ): Promise<SpendingGridRow[]>
  getCashSummary(start: string, end: string, profileId?: string): Promise<CashSummaryResponse>
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
  /** Portfolio summary. `asOf` (YYYY-MM-DD) reports balances as of that date (carry-forward); omitted = today. */
  getPortfolio(profileId?: string, asOf?: string): Promise<PortfolioResponse>
  getPortfolioHistory(
    start: string,
    end: string,
    granularity?: Granularity,
    profileId?: string
  ): Promise<PortfolioHistoryRow[]>
  getHoldings(accountId: string): Promise<Holding[]>
  getHoldingsBatch(accountIds: string[]): Promise<Holding[]>
  /** Per-holding value history for a single account. Maps to `GET /api/holdings/account-history`. */
  getAccountHoldingsHistory(
    accountId: string,
    start: string,
    end: string,
    granularity?: Granularity
  ): Promise<AccountHoldingsHistory>
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
  /** Hard-delete one transaction. Maps to `DELETE /api/transactions/:id`. */
  deleteTransaction(id: string): Promise<void>
  /** Hard-delete many transactions. Maps to `DELETE /api/transactions { ids }`. */
  bulkDeleteTransactions(ids: string[]): Promise<void>
  /** Assign one leaf category to many transactions. Maps to `PATCH /api/transactions { ids, category_id }`. */
  bulkSetCategory(ids: string[], categoryId: string): Promise<void>

  // ── Currencies ────────────────────────────────────────────────────
  getCurrencies(): Promise<Currency[]>
  createCurrency(body: { code: string; fx_rate: string }): Promise<Currency>
  updateCurrency(code: string, body: { fx_rate?: string; is_preferred?: boolean }): Promise<Currency>
  deleteCurrency(code: string): Promise<void>

  // ── Import ────────────────────────────────────────────────────────
  importCsv(accountId: string, file: File): Promise<ImportResult>

  /** Stage 1: upload files to `/api/parse`. Returns a preview with payloads.
   * Pass `opts.onProgress` to receive SSE progress events during the parse. */
  parseDocuments(
    files: File[],
    accountId: string,
    hints: ParseHints,
    opts?: ParseOptions,
  ): Promise<IngestionPreview>
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

  // ── Investments ───────────────────────────────────────────────────
  /** List investment events. Maps to `GET /api/investments`. */
  listInvestments(
    accountId?: string,
    symbol?: string,
    eventType?: string
  ): Promise<InvestmentEvent[]>
  /** Create one investment event. Maps to `POST /api/investments`. */
  createInvestment(body: CreateInvestmentEventBody): Promise<InvestmentEvent>
  /** Update an investment event. Maps to `PATCH /api/investments/:id`. */
  updateInvestment(id: string, body: PatchInvestmentEventBody): Promise<InvestmentEvent>
  /** Delete an investment event. Maps to `DELETE /api/investments/:id`. */
  deleteInvestment(id: string): Promise<void>
  /** S104 average-cost pool snapshot per symbol. Maps to `GET /api/investments/pools`. */
  getInvestmentPools(profileId?: string): Promise<S104PoolState[]>
  /** Cumulative net invested vs market value over time. Maps to `GET /api/investments/history`. */
  getInvestmentHistory(
    start: string,
    end: string,
    granularity: Granularity,
    profileId?: string,
    accountIds?: string[],
  ): Promise<InvestmentHistoryRow[]>

  // ── Documents ─────────────────────────────────────────────────────
  /**
   * List all stored source documents with their orphan flag. `reference_count`
   * is `null` here (computed lazily); fetch the real count per doc via
   * {@link getDocument}.
   */
  listDocuments(): Promise<DocumentSummary[]>
  /**
   * Fetch one document with its computed `reference_count`. Maps to
   * `GET /api/documents/:id`. Used to resolve the lazy per-row link count.
   */
  getDocument(id: string): Promise<DocumentSummary>
  /** Upload one or more standalone documents (origin = "manual"). */
  uploadDocuments(files: File[], accountId?: string): Promise<DocumentSummary[]>
  /**
   * Delete a document. Without `force`, a referenced document throws a
   * {@link DocumentReferencedError} carrying the per-entity breakdown so the UI
   * can confirm before retrying with `force = true`.
   */
  deleteDocument(id: string, force?: boolean): Promise<DocumentDeleteResult>
  /** Browser URL to download a stored document's bytes. */
  documentDownloadUrl(id: string): string
}

/**
 * Thrown by {@link ApiService.deleteDocument} when the document is still linked
 * to rows and `force` was not set. Carries the per-entity reference breakdown.
 */
export class DocumentReferencedError extends Error {
  readonly references: { transactions: number; holdings: number; investments: number }
  constructor(references: { transactions: number; holdings: number; investments: number }) {
    super("document is still referenced")
    this.name = "DocumentReferencedError"
    this.references = references
  }
}
