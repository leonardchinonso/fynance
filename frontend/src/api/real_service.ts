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
import type { ApiService, HoldingsImportResponse } from "./service"
import { MockApiService } from "./mock_service"

const BASE = "/api"

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
  if (!res.ok) {
    const body = await res.text()
    throw new Error(`${res.status} ${res.statusText}: ${body}`)
  }
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
  if (!res.ok) {
    const text = await res.text()
    throw new Error(`${res.status} ${res.statusText}: ${text}`)
  }
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
  if (!res.ok) {
    const text = await res.text()
    throw new Error(`${res.status} ${res.statusText}: ${text}`)
  }
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
  if (!res.ok) {
    const text = await res.text()
    throw new Error(`${res.status} ${res.statusText}: ${text}`)
  }
  return res.json()
}

async function del(path: string): Promise<void> {
  const token = getAuthToken()
  const headers: Record<string, string> = {}
  if (token) headers["Authorization"] = `Bearer ${token}`
  const res = await fetch(`${window.location.origin}${path}`, { method: "DELETE", headers })
  if (res.status === 401) throw new AuthError(!!token)
  if (!res.ok) {
    const text = await res.text()
    throw new Error(`${res.status} ${res.statusText}: ${text}`)
  }
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
  ): Promise<PaginatedResponse<Transaction>> {
    const params: Record<string, string> = {}
    if (filters.start) params.start = filters.start
    if (filters.end) params.end = filters.end
    if (filters.accounts?.length) params.accounts = filters.accounts.join(",")
    if (filters.categories?.length)
      params.categories = filters.categories.join(",")
    if (filters.search) params.search = filters.search
    if (filters.profile_id) params.profile_id = filters.profile_id
    if (filters.page) params.page = String(filters.page)
    if (filters.limit) params.limit = String(filters.limit)
    return get<PaginatedResponse<Transaction>>(`${BASE}/transactions`, params)
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
    if (filters.profile_id) params.profile_id = filters.profile_id
    if (filters.direction) params.direction = filters.direction
    const res = await get<{ preferred_currency: string; rows: CategoryTotal[] }>(`${BASE}/transactions/by-category`, params)
    return res.rows
  }

  async getCategories(): Promise<string[]> {
    const grouped = await get<Record<string, { id: string; name: string; children?: { id: string; name: string }[] }[]>>(`${BASE}/transactions/categories`)
    return Object.values(grouped).flat().flatMap(node => {
      const children = node.children ?? []
      if (children.length === 0) return [node.name]
      return children.map(c => `${node.name}: ${c.name}`)
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
    profileId?: string
  ): Promise<SpendingGridRow[]> {
    const params: Record<string, string> = { start, end, granularity }
    if (profileId) params.profile_id = profileId
    const res = await get<{ preferred_currency: string; rows: SpendingGridRow[] }>(`${BASE}/budget/spending-grid`, params)
    return res.rows
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

  async getHoldingsBatch(accountIds: string[]): Promise<Holding[]> {
    return get<Holding[]>(`${BASE}/holdings`, { account_ids: accountIds.join(",") })
  }

  async getCashFlow(
    start: string,
    end: string,
    granularity: Granularity = "monthly",
    profileId?: string
  ): Promise<CashFlowMonth[]> {
    const params: Record<string, string> = { start, end, granularity }
    if (profileId) params.profile_id = profileId
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
    // Backend returns HashMap<section, Vec<CategoryNode>> — flatten to a single array
    const grouped = await get<Record<string, CategoryNode[]>>(`${BASE}/categories`)
    return Object.values(grouped).flat()
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
    hints: ParseHints
  ): Promise<IngestionPreview> {
    const formData = new FormData()
    files.forEach((f) => formData.append("files[]", f, f.name))
    formData.append("account_id", accountId)
    formData.append("hints", JSON.stringify(hints))
    return postMultipart<IngestionPreview>(`${BASE}/parse`, formData)
  }

  async commitTransactions(payload: ImportPayload): Promise<ImportResult> {
    return post<ImportResult>(`${BASE}/transactions/import`, payload)
  }

  async commitHoldings(payload: HoldingsImportPayload): Promise<HoldingsImportResponse> {
    const res = await post<{ inserted?: number; updated?: number; total?: number; holdings_imported?: number; ok?: boolean }>(
      `${BASE}/holdings/import`,
      payload
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

  // ── Mock fallback (backend endpoint doesn't exist yet) ──────────

  async exportData(format: string): Promise<void> {
    return mock.exportData(format)
  }
}
