import { getAuthToken } from "./client"

export class AuthError extends Error {
  hasToken: boolean
  constructor(hasToken: boolean) {
    super(
      hasToken
        ? "Your token may be expired or invalid."
        : "Authorization required. You may need to generate an API token."
    )
    this.name = "AuthError"
    this.hasToken = hasToken
  }
}

/** Structured error thrown for non-2xx API responses. Carries the backend's
 * machine-readable `code` so callers can match on it without parsing strings. */
export class ApiError extends Error {
  status: number
  code: string
  constructor(status: number, code: string, message: string) {
    super(message)
    this.name = "ApiError"
    this.status = status
    this.code = code
  }
}

async function parseError(res: Response): Promise<ApiError> {
  const text = await res.text()
  try {
    const body = JSON.parse(text) as { error?: unknown; code?: unknown }
    const message = typeof body.error === "string" ? body.error : text
    const code = typeof body.code === "string" ? body.code : "unknown"
    return new ApiError(res.status, code, message)
  } catch {
    return new ApiError(res.status, "unknown", text || `${res.status} ${res.statusText}`)
  }
}

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
import type { Holding as HoldingRow } from "@/bindings/Holding"
import type { HoldingWrite } from "@/bindings/HoldingWrite"
import type { HoldingsWritePayload } from "@/bindings/HoldingsWritePayload"
import type { InvestmentsImportPayload } from "@/bindings/InvestmentsImportPayload"
import type { InvestmentImportResult } from "@/bindings/InvestmentImportResult"
import type { InvestmentEvent } from "@/bindings/InvestmentEvent"
import type { CreateInvestmentEventBody } from "@/bindings/CreateInvestmentEventBody"
import type { PatchInvestmentEventBody } from "@/bindings/PatchInvestmentEventBody"
import type { S104PoolState } from "@/bindings/S104PoolState"
import type { CapitalGainsResponse } from "@/bindings/CapitalGainsResponse"
import type { DocumentSummary } from "@/bindings/DocumentSummary"
import type { DocumentDeleteResult } from "@/bindings/DocumentDeleteResult"
import type { AccountHoldingsHistory, ApiService, CgtFilters, HoldingsImportResponse, ParseOptions, ParseProgressEvent } from "./service"
import { DocumentReferencedError } from "./service"
import { cgtFiltersToParams } from "./cgt_filter_params"
import { MockApiService } from "./mock_service"

const BASE = "/api"

// Holding types whose value is computed from quantity x price_per_unit; all
// others are scalar (cash, property, loan, credit).
const COMPUTED_HOLDING_TYPES = new Set(["stock", "etf", "fund", "bond", "crypto"])

// Map a flat Holding (parse/preview shape) to the write-API union: computed
// holdings send quantity+price, everything else sends a scalar value. The
// backend rejects payloads that set both arms.
function toHoldingWrite(h: HoldingRow): HoldingWrite {
  const base = {
    symbol: h.symbol,
    name: h.name,
    holding_type: h.holding_type,
    currency: h.currency,
    as_of: h.as_of,
    sub_account: h.sub_account ?? null,
    is_closed: h.is_closed,
    source_document_ids: h.source_document_ids,
  }
  if (COMPUTED_HOLDING_TYPES.has(h.holding_type) && h.price_per_unit != null) {
    return { ...base, quantity: h.quantity, price_per_unit: h.price_per_unit, value: null }
  }
  return { ...base, value: h.value, quantity: null, price_per_unit: null }
}

async function get<T>(path: string, params?: Record<string, string>): Promise<T> {
  const url = new URL(path, window.location.origin)
  if (params) {
    for (const [k, v] of Object.entries(params)) {
      if (v !== undefined && v !== "") url.searchParams.set(k, v)
    }
  }
  const token = getAuthToken()
  const headers: Record<string, string> = {}
  if (token) headers["Authorization"] = `Bearer ${token}`
  const res = await fetch(url.toString(), { headers })
  if (res.status === 401) throw new AuthError(!!token)
  if (!res.ok) throw await parseError(res)
  return res.json()
}

async function post<T>(path: string, body: unknown): Promise<T> {
  const token = getAuthToken()
  const headers: Record<string, string> = { "Content-Type": "application/json" }
  if (token) headers["Authorization"] = `Bearer ${token}`
  const res = await fetch(`${window.location.origin}${path}`, {
    method: "POST",
    headers,
    body: JSON.stringify(body),
  })
  if (res.status === 401) throw new AuthError(!!token)
  if (!res.ok) throw await parseError(res)
  return res.json()
}

async function postMultipart<T>(path: string, formData: FormData): Promise<T> {
  const token = getAuthToken()
  const headers: Record<string, string> = {}
  if (token) headers["Authorization"] = `Bearer ${token}`
  const res = await fetch(`${window.location.origin}${path}`, {
    method: "POST",
    headers,
    body: formData,
  })
  if (res.status === 401) throw new AuthError(!!token)
  if (!res.ok) throw await parseError(res)
  return res.json()
}

async function patch<T>(path: string, body: unknown): Promise<T> {
  const token = getAuthToken()
  const headers: Record<string, string> = { "Content-Type": "application/json" }
  if (token) headers["Authorization"] = `Bearer ${token}`
  const res = await fetch(`${window.location.origin}${path}`, {
    method: "PATCH",
    headers,
    body: JSON.stringify(body),
  })
  if (res.status === 401) throw new AuthError(!!token)
  if (!res.ok) throw await parseError(res)
  return res.json()
}

async function del(path: string): Promise<void> {
  const token = getAuthToken()
  const headers: Record<string, string> = {}
  if (token) headers["Authorization"] = `Bearer ${token}`
  const res = await fetch(`${window.location.origin}${path}`, { method: "DELETE", headers })
  if (res.status === 401) throw new AuthError(!!token)
  if (!res.ok) throw await parseError(res)
}

// Mock fallback for endpoints the backend doesn't have yet
const mock = new MockApiService()

/**
 * RealApiService calls the Rust backend for every endpoint that has
 * server-side support. The only remaining mock fallback is exportData
 * which isn't built on the backend yet.
 */
export class RealApiService implements ApiService {
  // ── Real endpoints ──────────────────────────────────────────────

  async getProfiles(): Promise<Profile[]> {
    return get<Profile[]>(`${BASE}/profiles`)
  }

  async getTransactions(
    filters: TransactionFilters
  ): Promise<Paginated<Transaction>> {
    const params: Record<string, string> = {}
    if (filters.start) params.start = filters.start
    if (filters.end) params.end = filters.end
    if (filters.accounts?.length) params.accounts = filters.accounts.join(",")
    if (filters.categories?.length)
      params.categories = filters.categories.join(",")
    if (filters.category_types?.length) params.category_types = filters.category_types.join(",")
    if (filters.search) params.search = filters.search
    if (filters.profile_id) params.profile_id = filters.profile_id
    if (filters.page) params.page = String(filters.page)
    if (filters.limit) params.limit = String(filters.limit)
    if (filters.sort) params.sort = filters.sort
    if (filters.sort_dir) params.sort_dir = filters.sort_dir
    return get<Paginated<Transaction>>(`${BASE}/transactions`, params)
  }

  async getTransactionsByCategory(
    filters: CategoryTotalFilters
  ): Promise<CategoryTotal[]> {
    const params: Record<string, string> = {}
    if (filters.start) params.start = filters.start
    if (filters.end) params.end = filters.end
    if (filters.accounts?.length) params.accounts = filters.accounts.join(",")
    if (filters.categories?.length)
      params.categories = filters.categories.join(",")
    if (filters.category_types?.length) params.category_types = filters.category_types.join(",")
    if (filters.profile_id) params.profile_id = filters.profile_id
    if (filters.direction) params.direction = filters.direction
    const res = await get<{ preferred_currency: string; rows: CategoryTotal[] }>(`${BASE}/transactions/by-category`, params)
    return res.rows
  }

  async getCategories(): Promise<string[]> {
    const nodes = await get<CategoryNode[]>(`${BASE}/transactions/categories`)
    return nodes.flatMap(node => {
      const children = node.children ?? []
      if (children.length === 0) return [node.name]
      return children.map(c => `${node.name}: ${c.name}`)
    })
  }

  async getCategoriesWithIds(): Promise<Array<{ id: string; name: string }>> {
    const nodes = await get<CategoryNode[]>(`${BASE}/transactions/categories`)
    return nodes.flatMap(node => {
      const children = node.children ?? []
      if (children.length === 0) return [{ id: node.id, name: node.name }]
      return children.map(c => ({ id: c.id, name: `${node.name}: ${c.name}` }))
    })
  }

  async getAccounts(profileId?: string): Promise<Account[]> {
    const params: Record<string, string> = {}
    if (profileId) params.profile_id = profileId
    return get<Account[]>(`${BASE}/accounts`, params)
  }

  async getBudget(month: string): Promise<BudgetRow[]> {
    const res = await get<{ preferred_currency: string; rows: BudgetRow[] }>(`${BASE}/budget/${month}`)
    return res.rows
  }

  async getSpendingGrid(
    start: string,
    end: string,
    granularity: Granularity,
    profileId?: string,
    filters?: SpendingGridFilters
  ): Promise<SpendingGridRow[]> {
    const params: Record<string, string> = { start, end, granularity }
    if (profileId) params.profile_id = profileId
    if (filters?.accounts?.length) params.accounts = filters.accounts.join(",")
    if (filters?.categories?.length) params.categories = filters.categories.join(",")
    if (filters?.categoryTypes?.length) params.category_types = filters.categoryTypes.join(",")
    if (filters?.groupBy) params.group_by = filters.groupBy
    const res = await get<{ preferred_currency: string; rows: SpendingGridRow[] }>(`${BASE}/budget/spending-grid`, params)
    return res.rows
  }

  async getCashSummary(start: string, end: string, profileId?: string): Promise<CashSummaryResponse> {
    const params: Record<string, string> = { start, end }
    if (profileId) params.profile_id = profileId
    return get<CashSummaryResponse>(`${BASE}/budget/cash-summary`, params)
  }

  async setStandingBudget(body: SetStandingBudgetBody): Promise<void> {
    await post(`${BASE}/budget`, body)
  }

  async setBudgetOverride(body: SetBudgetOverrideBody): Promise<void> {
    await post(`${BASE}/budget/override`, body)
  }

  // ── Portfolio endpoints (now backed by the real backend) ────────

  async getPortfolio(profileId?: string): Promise<PortfolioResponse> {
    const params: Record<string, string> = {}
    if (profileId) params.profile_id = profileId
    return get<PortfolioResponse>(`${BASE}/holdings/summary`, params)
  }

  async getPortfolioHistory(
    start: string,
    end: string,
    granularity: Granularity = "monthly",
    profileId?: string
  ): Promise<PortfolioHistoryRow[]> {
    const params: Record<string, string> = { start, end, granularity }
    if (profileId) params.profile_id = profileId
    const res = await get<{ preferred_currency: string; rows: PortfolioHistoryRow[] }>(`${BASE}/holdings/history`, params)
    return res.rows
  }

  async getHoldings(accountId: string): Promise<Holding[]> {
    return get<Holding[]>(`${BASE}/holdings`, { account_id: accountId })
  }

  async getAccountHoldingsHistory(
    accountId: string,
    start: string,
    end: string,
    granularity: Granularity = "monthly"
  ): Promise<AccountHoldingsHistory> {
    return get<AccountHoldingsHistory>(`${BASE}/holdings/account-history`, {
      account_id: accountId,
      start,
      end,
      granularity,
    })
  }

  async getHoldingsBatch(accountIds: string[]): Promise<Holding[]> {
    return get<Holding[]>(`${BASE}/holdings`, { account_ids: accountIds.join(",") })
  }

  async getCashFlow(
    start: string,
    end: string,
    granularity: Granularity = "monthly",
    profileId?: string,
    excludeCategoryIds?: string[]
  ): Promise<CashFlowMonth[]> {
    const params: Record<string, string> = { start, end, granularity }
    if (profileId) params.profile_id = profileId
    if (excludeCategoryIds && excludeCategoryIds.length > 0) {
      params.exclude_category_ids = excludeCategoryIds.join(",")
    }
    const res = await get<{ preferred_currency: string; rows: CashFlowMonth[] }>(`${BASE}/holdings/cash-flow`, params)
    return res.rows
  }

  async getAccountBalances(
    start: string,
    end: string,
    _profileId?: string
  ): Promise<AccountSnapshot[]> {
    return get<AccountSnapshot[]>(`${BASE}/holdings/balances`, { start, end })
  }

  // ── Settings / CRUD ──────────────────────────────────────────────

  async createProfile(body: { id: string; name: string }): Promise<Profile> {
    return post<Profile>(`${BASE}/profiles`, body)
  }

  async updateProfile(id: string, body: { name?: string }): Promise<Profile> {
    return patch<Profile>(`${BASE}/profiles/${encodeURIComponent(id)}`, body)
  }

  async deleteProfile(id: string): Promise<void> {
    return del(`${BASE}/profiles/${encodeURIComponent(id)}`)
  }

  async createAccount(body: CreateAccountBody): Promise<Account> {
    return post<Account>(`${BASE}/accounts`, body)
  }

  async updateAccount(id: string, body: PatchAccountBody): Promise<Account> {
    return patch<Account>(`${BASE}/accounts/${encodeURIComponent(id)}`, body)
  }

  async deleteAccount(id: string): Promise<void> {
    return del(`${BASE}/accounts/${encodeURIComponent(id)}`)
  }

  async getCategoryDetails(): Promise<CategoryNode[]> {
    return get<CategoryNode[]>(`${BASE}/categories`)
  }

  async createCategory(body: CreateCategoryBody): Promise<Category> {
    return post<Category>(`${BASE}/categories`, body)
  }

  async updateCategory(id: string, body: PatchCategoryBody): Promise<Category> {
    return patch<Category>(`${BASE}/categories/${id}`, body)
  }

  async deleteCategory(id: string): Promise<void> {
    return del(`${BASE}/categories/${id}`)
  }

  async patchTransaction(id: string, body: PatchTransactionBody): Promise<Transaction> {
    return patch<Transaction>(`${BASE}/transactions/${id}`, body)
  }

  // ── Import ────────────────────────────────────────────────────────

  async importCsv(accountId: string, file: File): Promise<ImportResult> {
    const formData = new FormData()
    formData.append("file", file)
    return postMultipart<ImportResult>(
      `${BASE}/import/csv?account=${encodeURIComponent(accountId)}`,
      formData
    )
  }

  async parseDocuments(
    files: File[],
    accountId: string,
    hints: ParseHints,
    opts?: ParseOptions
  ): Promise<IngestionPreview> {
    const parseId = opts?.parseId ?? crypto.randomUUID()
    const formData = new FormData()
    files.forEach((f) => formData.append("files[]", f, f.name))
    formData.append("account_id", accountId)
    formData.append("hints", JSON.stringify(hints))
    formData.append("parse_id", parseId)

    // Subscribe to progress before firing the upload. The backend registers the
    // channel at the start of POST /api/parse and polls briefly for late
    // subscribers, so opening the stream here reliably attaches.
    let es: EventSource | null = null
    if (opts?.onProgress) {
      const onProgress = opts.onProgress
      es = new EventSource(`${BASE}/parse/progress/${encodeURIComponent(parseId)}`)
      const forward = (ev: MessageEvent) => {
        // Generic transport errors arrive with no data; only named SSE frames carry JSON.
        if (typeof ev.data !== "string") return
        try {
          onProgress(JSON.parse(ev.data) as ParseProgressEvent)
        } catch {
          // ignore malformed frame
        }
      }
      for (const name of ["phase", "llm_start", "llm_progress", "done", "error"]) {
        es.addEventListener(name, forward as EventListener)
      }
    }

    try {
      return await postMultipart<IngestionPreview>(`${BASE}/parse`, formData)
    } finally {
      // We own the close, so the browser never auto-reconnects into a not_found loop.
      es?.close()
    }
  }

  async commitTransactions(payload: ImportPayload): Promise<ImportResult> {
    return post<ImportResult>(`${BASE}/transactions/import`, payload)
  }

  async commitHoldings(payload: HoldingsImportPayload): Promise<HoldingsImportResponse> {
    const writePayload: HoldingsWritePayload = {
      account_id: payload.account_id,
      holdings: (payload.holdings ?? []).map(toHoldingWrite),
    }
    const res = await post<{ inserted?: number; updated?: number; total?: number; holdings_imported?: number; ok?: boolean }>(
      `${BASE}/holdings/import`,
      writePayload
    )
    return {
      inserted: res.inserted ?? res.holdings_imported ?? 0,
      updated: res.updated ?? 0,
      total: res.total ?? res.holdings_imported ?? (payload.holdings?.length ?? 0),
    }
  }

  async commitInvestments(payload: InvestmentsImportPayload): Promise<InvestmentImportResult> {
    return post<InvestmentImportResult>(`${BASE}/investments/import`, payload)
  }

  // ── Reports ───────────────────────────────────────────────────────

  async getCapitalGains(filters: CgtFilters): Promise<CapitalGainsResponse> {
    const params = cgtFiltersToParams(filters)
    return get<CapitalGainsResponse>(`${BASE}/investments/capital-gains`, params)
  }

  // ── Investments ───────────────────────────────────────────────────

  async listInvestments(
    accountId?: string,
    symbol?: string,
    eventType?: string
  ): Promise<InvestmentEvent[]> {
    const params: Record<string, string> = {}
    if (accountId) params.account_id = accountId
    if (symbol) params.symbol = symbol
    if (eventType) params.event_type = eventType
    return get<InvestmentEvent[]>(`${BASE}/investments`, params)
  }

  async createInvestment(body: CreateInvestmentEventBody): Promise<InvestmentEvent> {
    return post<InvestmentEvent>(`${BASE}/investments`, body)
  }

  async updateInvestment(id: string, body: PatchInvestmentEventBody): Promise<InvestmentEvent> {
    return patch<InvestmentEvent>(`${BASE}/investments/${encodeURIComponent(id)}`, body)
  }

  async deleteInvestment(id: string): Promise<void> {
    return del(`${BASE}/investments/${encodeURIComponent(id)}`)
  }

  async getInvestmentPools(profileId?: string): Promise<S104PoolState[]> {
    const qs = profileId ? `?profile_ids=${encodeURIComponent(profileId)}` : ""
    return get<S104PoolState[]>(`${BASE}/investments/pools${qs}`)
  }

  // ── Currencies ────────────────────────────────────────────────────

  async getCurrencies(): Promise<Currency[]> {
    return get<Currency[]>(`${BASE}/currencies`)
  }

  async createCurrency(body: { code: string; fx_rate: string }): Promise<Currency> {
    return post<Currency>(`${BASE}/currencies`, body)
  }

  async updateCurrency(code: string, body: { fx_rate?: string; is_preferred?: boolean }): Promise<Currency> {
    return patch<Currency>(`${BASE}/currencies/${code}`, body)
  }

  async deleteCurrency(code: string): Promise<void> {
    return del(`${BASE}/currencies/${code}`)
  }

  // ── Documents ─────────────────────────────────────────────────────

  async listDocuments(): Promise<DocumentSummary[]> {
    return get<DocumentSummary[]>(`${BASE}/documents`)
  }

  async getDocument(id: string): Promise<DocumentSummary> {
    return get<DocumentSummary>(`${BASE}/documents/${encodeURIComponent(id)}`)
  }

  async uploadDocuments(files: File[], accountId?: string): Promise<DocumentSummary[]> {
    const formData = new FormData()
    files.forEach((f) => formData.append("files[]", f, f.name))
    if (accountId) formData.append("account_id", accountId)
    return postMultipart<DocumentSummary[]>(`${BASE}/documents`, formData)
  }

  async deleteDocument(id: string, force = false): Promise<DocumentDeleteResult> {
    const token = getAuthToken()
    const headers: Record<string, string> = {}
    if (token) headers["Authorization"] = `Bearer ${token}`
    const url = `${BASE}/documents/${encodeURIComponent(id)}${force ? "?force=true" : ""}`
    const res = await fetch(`${window.location.origin}${url}`, { method: "DELETE", headers })
    if (res.status === 401) throw new AuthError(!!token)
    if (res.status === 409) {
      const body = (await res.json().catch(() => ({}))) as {
        references?: { transactions?: number; holdings?: number; investments?: number }
      }
      const r = body.references ?? {}
      throw new DocumentReferencedError({
        transactions: r.transactions ?? 0,
        holdings: r.holdings ?? 0,
        investments: r.investments ?? 0,
      })
    }
    if (!res.ok) throw await parseError(res)
    return res.json()
  }

  documentDownloadUrl(id: string): string {
    return `${BASE}/documents/${encodeURIComponent(id)}/download`
  }

  // ── Mock fallback (backend endpoint doesn't exist yet) ──────────

  async exportData(format: string): Promise<void> {
    return mock.exportData(format)
  }
}
