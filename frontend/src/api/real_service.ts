import type {
  Account,
  BudgetRow,
  BudgetUpdateRequest,
  CashFlowMonth,
  CategoryTotal,
  CategoryTotalFilters,
  Granularity,
  Holding,
  PaginatedResponse,
  PortfolioHistoryRow,
  PortfolioResponse,
  PortfolioSnapshot,
  Profile,
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

// Mock fallback for endpoints the backend doesn't have yet
const mock = new MockApiService()

/**
 * RealApiService calls the Rust backend for endpoints that exist,
 * and falls back to MockApiService for portfolio/export endpoints
 * that haven't been built yet.
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

  async updateBudget(req: BudgetUpdateRequest): Promise<void> {
    await post(`${BASE}/budget`, {
      category: req.category,
      amount: req.amount,
    })
  }

  // ── Mock fallbacks (backend endpoints don't exist yet) ──────────

  async getPortfolio(profileId?: string): Promise<PortfolioResponse> {
    return mock.getPortfolio(profileId)
  }

  async getPortfolioHistory(
    start?: string,
    end?: string
  ): Promise<PortfolioHistoryRow[]> {
    return mock.getPortfolioHistory(start, end)
  }

  async getHoldings(accountId: string): Promise<Holding[]> {
    return mock.getHoldings(accountId)
  }

  async getCashFlow(start?: string, end?: string): Promise<CashFlowMonth[]> {
    return mock.getCashFlow(start, end)
  }

  async getAccountSnapshots(
    start?: string,
    end?: string
  ): Promise<PortfolioSnapshot[]> {
    return mock.getAccountSnapshots(start, end)
  }

  async exportData(format: string): Promise<void> {
    return mock.exportData(format)
  }
}
