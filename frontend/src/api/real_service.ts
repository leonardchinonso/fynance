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
import type { ApiService } from "./service"
import { MockApiService } from "./mock_service"

const BASE = "/api"

async function get<T>(path: string, params?: Record<string, string>): Promise<T> {
  const url = new URL(path, window.location.origin)
  if (params) {
    for (const [k, v] of Object.entries(params)) {
      if (v !== undefined && v !== "") url.searchParams.set(k, v)
    }
  }
  const res = await fetch(url.toString())
  if (!res.ok) {
    const body = await res.text()
    throw new Error(`${res.status} ${res.statusText}: ${body}`)
  }
  return res.json()
}

async function post<T>(path: string, body: unknown): Promise<T> {
  const res = await fetch(`${window.location.origin}${path}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  })
  if (!res.ok) {
    const text = await res.text()
    throw new Error(`${res.status} ${res.statusText}: ${text}`)
  }
  return res.json()
}

async function postMultipart<T>(path: string, formData: FormData): Promise<T> {
  const res = await fetch(`${window.location.origin}${path}`, {
    method: "POST",
    body: formData,
  })
  if (!res.ok) {
    const text = await res.text()
    throw new Error(`${res.status} ${res.statusText}: ${text}`)
  }
  return res.json()
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
    return get<CategoryTotal[]>(`${BASE}/transactions/by-category`, params)
  }

  async getCategories(): Promise<string[]> {
    return get<string[]>(`${BASE}/transactions/categories`)
  }

  async getAccounts(profileId?: string): Promise<Account[]> {
    const params: Record<string, string> = {}
    if (profileId) params.profile_id = profileId
    return get<Account[]>(`${BASE}/accounts`, params)
  }

  async getBudget(month: string): Promise<BudgetRow[]> {
    return get<BudgetRow[]>(`${BASE}/budget/${month}`)
  }

  async getSpendingGrid(
    start: string,
    end: string,
    granularity: Granularity,
    profileId?: string
  ): Promise<SpendingGridRow[]> {
    const params: Record<string, string> = { start, end, granularity }
    if (profileId) params.profile_id = profileId
    return get<SpendingGridRow[]>(`${BASE}/budget/spending-grid`, params)
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
    return get<PortfolioResponse>(`${BASE}/portfolio`, params)
  }

  async getPortfolioHistory(
    start: string,
    end: string,
    granularity: Granularity = "monthly",
    profileId?: string
  ): Promise<PortfolioHistoryRow[]> {
    const params: Record<string, string> = { start, end, granularity }
    if (profileId) params.profile_id = profileId
    return get<PortfolioHistoryRow[]>(`${BASE}/portfolio/history`, params)
  }

  async getHoldings(accountId: string): Promise<Holding[]> {
    return get<Holding[]>(`${BASE}/holdings`, { account_id: accountId })
  }

  async getCashFlow(
    start: string,
    end: string,
    granularity: Granularity = "monthly",
    profileId?: string
  ): Promise<CashFlowMonth[]> {
    const params: Record<string, string> = { start, end, granularity }
    if (profileId) params.profile_id = profileId
    return get<CashFlowMonth[]>(`${BASE}/cash-flow`, params)
  }

  async getAccountBalances(
    start: string,
    end: string,
    _profileId?: string
  ): Promise<AccountSnapshot[]> {
    // Backend endpoint is /api/portfolio/balances. The non-summary mode
    // returns the full per-account snapshot list; omit ?summary=true.
    return get<AccountSnapshot[]>(`${BASE}/portfolio/balances`, {
      start,
      end,
    })
  }

  // ── Settings / CRUD ──────────────────────────────────────────────

  async createProfile(body: { id: string; name: string }): Promise<Profile> {
    return post<Profile>(`${BASE}/profiles`, body)
  }

  async createAccount(body: CreateAccountBody): Promise<Account> {
    return post<Account>(`${BASE}/accounts`, body)
  }

  // Categories: mock fallback until BE adds category CRUD
  async getCategoryDetails(): Promise<CategoryDetail[]> {
    return mock.getCategoryDetails()
  }

  async createCategory(body: { name: string; description: string; group: string }): Promise<CategoryDetail> {
    return mock.createCategory(body)
  }

  async updateCategory(id: string, body: { name?: string; description?: string; group?: string }): Promise<CategoryDetail> {
    return mock.updateCategory(id, body)
  }

  async deleteCategory(id: string): Promise<void> {
    return mock.deleteCategory(id)
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

  // ── Mock fallback (backend endpoint doesn't exist yet) ──────────

  async exportData(format: string): Promise<void> {
    return mock.exportData(format)
  }
}
