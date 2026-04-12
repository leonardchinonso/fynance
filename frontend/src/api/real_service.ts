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
    // Backend caps limit at 200. For requests that want more (e.g. charts
    // asking for "all" transactions via limit=10000), paginate transparently.
    const BACKEND_MAX = 200
    const wantLimit = filters.limit ?? 25

    const buildParams = (page: number, limit: number) => {
      const params: Record<string, string> = { page: String(page), limit: String(limit) }
      if (filters.start) params.start = filters.start
      if (filters.end) params.end = filters.end
      if (filters.accounts?.length) params.accounts = filters.accounts.join(",")
      if (filters.categories?.length)
        params.categories = filters.categories.join(",")
      if (filters.search) params.search = filters.search
      if (filters.profile_id) params.profile_id = filters.profile_id
      return params
    }

    if (wantLimit <= BACKEND_MAX) {
      return get<PaginatedResponse<Transaction>>(
        `${BASE}/transactions`,
        buildParams(filters.page ?? 1, wantLimit)
      )
    }

    // Loop through pages until we have wantLimit rows or exhaust the result set
    const all: Transaction[] = []
    let page = 1
    let total = 0
    while (all.length < wantLimit) {
      const res = await get<PaginatedResponse<Transaction>>(
        `${BASE}/transactions`,
        buildParams(page, BACKEND_MAX)
      )
      total = res.total
      all.push(...res.data)
      if (res.data.length < BACKEND_MAX) break
      if (all.length >= total) break
      page++
    }
    return { data: all.slice(0, wantLimit), total, page: 1, limit: wantLimit }
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

  async getAccountBalances(
    start?: string,
    end?: string
  ): Promise<AccountSnapshot[]> {
    return mock.getAccountBalances(start, end)
  }

  async exportData(format: string): Promise<void> {
    return mock.exportData(format)
  }
}
