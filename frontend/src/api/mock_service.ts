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
import {
  MOCK_PROFILES,
  MOCK_ACCOUNTS,
  MOCK_TRANSACTIONS,
  MOCK_HOLDINGS,
  MOCK_BUDGETS,
  MOCK_ACCOUNT_BALANCES,
} from "@/data"
import { delay, getMonthFromDate, getMonthsInRange } from "@/lib/utils"

const DELAY_MS = 1000

const mockCurrencies: Currency[] = [
  { code: "GBP", is_preferred: true, fx_rate: "1", updated_at: null },
  { code: "NGN", is_preferred: false, fx_rate: "0.00051", updated_at: "2026-04-01T10:00:00Z" },
  { code: "USD", is_preferred: false, fx_rate: "0.79", updated_at: "2026-03-15T08:00:00Z" },
]

// Available/unavailable account type classification
const AVAILABLE_TYPES = new Set(["checking", "savings", "investment", "cash", "credit"])
// Liability types that subtract from unavailable wealth (e.g. mortgage offsets property value)
const UNAVAILABLE_LIABILITY_TYPES = new Set(["mortgage"])

/**
 * Splits an account balance into available / unavailable wealth. Shared by
 * the portfolio summary and history so they cannot diverge.
 */
function classifyBalance(
  accountType: string,
  bal: number
): { available: number; unavailable: number } {
  if (AVAILABLE_TYPES.has(accountType)) {
    return { available: accountType === "credit" ? -bal : bal, unavailable: 0 }
  }
  if (UNAVAILABLE_LIABILITY_TYPES.has(accountType)) {
    return { available: 0, unavailable: -bal }
  }
  return { available: 0, unavailable: bal }
}

export class MockApiService implements ApiService {
  async getProfiles(): Promise<Profile[]> {
    await delay(DELAY_MS)
    return MOCK_PROFILES
  }

  async getTransactions(
    filters: TransactionFilters
  ): Promise<PaginatedResponse<Transaction>> {
    await delay(DELAY_MS)

    let data = [...MOCK_TRANSACTIONS]

    // Filter by profile (via account ownership)
    if (filters.profile_id) {
      const profileAccounts = new Set(
        MOCK_ACCOUNTS.filter((a) => a.profile_ids.includes(filters.profile_id!)).map(
          (a) => a.id
        )
      )
      data = data.filter((t) => profileAccounts.has(t.account_id))
    }

    if (filters.start) {
      data = data.filter((t) => t.date >= filters.start!)
    }
    if (filters.end) {
      data = data.filter((t) => t.date <= filters.end!)
    }
    if (filters.accounts && filters.accounts.length > 0) {
      const set = new Set(filters.accounts)
      data = data.filter((t) => set.has(t.account_id))
    }
    if (filters.categories && filters.categories.length > 0) {
      const set = new Set(filters.categories)
      data = data.filter((t) => t.category !== null && set.has(t.category))
    }
    if (filters.search) {
      const q = filters.search.toLowerCase()
      data = data.filter(
        (t) =>
          t.normalized.toLowerCase().includes(q) ||
          t.description.toLowerCase().includes(q) ||
          (t.category ?? "").toLowerCase().includes(q) ||
          t.account_id.toLowerCase().includes(q) ||
          (t.notes ?? "").toLowerCase().includes(q)
      )
    }

    const total = data.length
    const page = filters.page ?? 1
    const limit = filters.limit ?? 25
    const start = (page - 1) * limit
    const paged = data.slice(start, start + limit)

    return { data: paged, total, page, limit }
  }

  /**
   * Mock of the backend `/api/transactions/by-category` aggregation.
   * Mirrors the server logic: group by leaf category, sum amounts,
   * honour direction (outflow = abs(negatives), income = positives),
   * and apply the same filters the real endpoint supports.
   */
  async getTransactionsByCategory(
    filters: CategoryTotalFilters
  ): Promise<CategoryTotal[]> {
    await delay(DELAY_MS)

    let data = [...MOCK_TRANSACTIONS]

    // Same filter order and semantics as getTransactions
    if (filters.profile_id) {
      const profileAccounts = new Set(
        MOCK_ACCOUNTS.filter((a) => a.profile_ids.includes(filters.profile_id!)).map(
          (a) => a.id
        )
      )
      data = data.filter((t) => profileAccounts.has(t.account_id))
    }
    if (filters.start) data = data.filter((t) => t.date >= filters.start!)
    if (filters.end) data = data.filter((t) => t.date <= filters.end!)
    if (filters.accounts && filters.accounts.length > 0) {
      const set = new Set(filters.accounts)
      data = data.filter((t) => set.has(t.account_id))
    }
    if (filters.categories && filters.categories.length > 0) {
      const set = new Set(filters.categories)
      data = data.filter((t) => t.category !== null && set.has(t.category))
    }

    // Direction filter
    if (filters.direction === "outflow") {
      data = data.filter((t) => parseFloat(t.amount) < 0)
    } else if (filters.direction === "income") {
      data = data.filter((t) => parseFloat(t.amount) > 0)
    }

    // Group by leaf category, summing by direction semantics
    const totals = new Map<string, number>()
    for (const t of data) {
      if (!t.category) continue
      const amt = parseFloat(t.amount)
      const contribution = filters.direction ? Math.abs(amt) : amt
      totals.set(t.category, (totals.get(t.category) ?? 0) + contribution)
    }

    // DESC order to match the backend's ORDER BY total DESC
    return Array.from(totals.entries())
      .map(([category, total]) => ({ category, total: total.toFixed(2), display_currency: null }))
      .sort((a, b) => parseFloat(b.total) - parseFloat(a.total))
  }

  async getCategories(): Promise<string[]> {
    await delay(DELAY_MS)
    const cats = new Set<string>()
    for (const t of MOCK_TRANSACTIONS) {
      if (t.category) cats.add(t.category)
    }
    return Array.from(cats).sort()
  }

  async getAccounts(profileId?: string): Promise<Account[]> {
    await delay(DELAY_MS)
    if (profileId) {
      return MOCK_ACCOUNTS.filter((a) => a.profile_ids.includes(profileId!))
    }
    return MOCK_ACCOUNTS
  }

  async getBudget(month: string): Promise<BudgetRow[]> {
    await delay(DELAY_MS)

    const budgets = MOCK_BUDGETS.filter((b) => b.month === month)

    // Calculate actual spending per category for this month
    const spending = new Map<string, number>()
    for (const t of MOCK_TRANSACTIONS) {
      if (getMonthFromDate(t.date) !== month) continue
      const amt = parseFloat(t.amount)
      if (amt >= 0) continue // skip income
      if (!t.category) continue
      spending.set(
        t.category,
        (spending.get(t.category) ?? 0) + Math.abs(amt)
      )
    }

    return budgets.map((b) => {
      const actual = spending.get(b.category) ?? 0
      const budgeted = parseFloat(b.amount)
      return {
        category: b.category,
        category_id: null,
        budgeted: b.amount,
        actual: actual.toFixed(2),
        actual_display: null,
        percent: budgeted > 0 ? Math.round((actual / budgeted) * 100) : 0,
      }
    })
  }

  async getSpendingGrid(
    start: string,
    end: string,
    _granularity: Granularity,
    profileId?: string
  ): Promise<SpendingGridRow[]> {
    await delay(DELAY_MS)

    const months = getMonthsInRange(start, end)

    // Get accounts for profile filtering
    let profileAccounts: Set<string> | null = null
    if (profileId) {
      profileAccounts = new Set(
        MOCK_ACCOUNTS.filter((a) => a.profile_ids.includes(profileId)).map((a) => a.id)
      )
    }

    // Group transactions by category and month
    const grid = new Map<string, Map<string, number>>()
    for (const t of MOCK_TRANSACTIONS) {
      if (t.date < start || t.date > end) continue
      if (profileAccounts && !profileAccounts.has(t.account_id)) continue
      const cat = t.category ?? "Other: Uncategorized"
      const month = getMonthFromDate(t.date)
      if (!grid.has(cat)) grid.set(cat, new Map())
      const catMap = grid.get(cat)!
      catMap.set(month, (catMap.get(month) ?? 0) + parseFloat(t.amount))
    }

    // Determine section based on category
    function getSection(cat: string): string {
      if (cat.startsWith("Income")) return "Income"
      if (
        cat.startsWith("Housing") ||
        cat.startsWith("Finance: Insurance") ||
        cat.startsWith("Entertainment: Streaming")
      )
        return "Bills"
      if (
        cat.startsWith("Finance: Savings") ||
        cat.startsWith("Finance: Investment")
      )
        return "Transfers"
      if (cat.startsWith("Travel")) return "Irregular"
      return "Spending"
    }

    const rows: SpendingGridRow[] = []
    for (const [cat, catMap] of grid) {
      const monthValues: Record<string, string | null> = {}
      let total = 0
      let monthsWithData = 0
      for (const m of months) {
        if (catMap.has(m)) {
          const val = catMap.get(m)!
          monthValues[m] = val.toFixed(2)
          total += val
          monthsWithData++
        } else {
          monthValues[m] = null
        }
      }
      const avg = monthsWithData > 0 ? total / monthsWithData : 0

      // Find budget for this category
      const budget = MOCK_BUDGETS.find((b) => b.category === cat)

      rows.push({
        category: cat,
        category_id: null,
        section: getSection(cat),
        periods: monthValues,
        periods_display: {},
        average: avg.toFixed(2),
        average_display: null,
        budget: budget?.amount ?? null,
        total: total.toFixed(2),
        total_display: null,
      })
    }

    // Sort by section order
    const sectionOrder = ["Income", "Bills", "Spending", "Irregular", "Transfers"]
    rows.sort(
      (a, b) =>
        sectionOrder.indexOf(a.section) - sectionOrder.indexOf(b.section) ||
        a.category.localeCompare(b.category)
    )

    return rows
  }

  /**
   * Mock of `POST /api/budget` - sets a standing budget that applies to
   * every month unless overridden. Stored in the shared MOCK_BUDGETS
   * array as a month-less row (empty month) so the mock mirrors the
   * backend's standing_budgets table.
   */
  async setStandingBudget(body: SetStandingBudgetBody): Promise<void> {
    await delay(DELAY_MS)
    const categoryKey = body.category_id ?? "Unknown"
    const existing = MOCK_BUDGETS.find(
      (b) => b.month === "" && b.category === categoryKey
    )
    if (existing) {
      existing.amount = body.amount
    } else {
      MOCK_BUDGETS.push({ month: "", category: categoryKey, amount: body.amount })
    }
  }

  async setBudgetOverride(body: SetBudgetOverrideBody): Promise<void> {
    await delay(DELAY_MS)
    const categoryKey = body.category_id ?? "Unknown"
    const existing = MOCK_BUDGETS.find(
      (b) => b.month === body.month && b.category === categoryKey
    )
    if (existing) {
      existing.amount = body.amount
    } else {
      MOCK_BUDGETS.push({ month: body.month, category: categoryKey, amount: body.amount })
    }
  }

  async getPortfolio(profileId?: string): Promise<PortfolioResponse> {
    await delay(DELAY_MS)

    const accounts = profileId
      ? MOCK_ACCOUNTS.filter((a) => a.profile_ids.includes(profileId!))
      : MOCK_ACCOUNTS

    let totalAssets = 0
    let totalLiabilities = 0
    let availableWealth = 0
    let unavailableWealth = 0

    for (const a of accounts) {
      const bal = parseFloat(a.balance ?? "0")
      if (a.type === "credit" && bal > 0) {
        totalLiabilities += bal
      } else {
        totalAssets += Math.abs(bal)
      }
      const split = classifyBalance(a.type, bal)
      availableWealth += split.available
      unavailableWealth += split.unavailable
    }

    const netWorth = totalAssets - totalLiabilities

    // By type
    const byType = new Map<string, number>()
    for (const a of accounts) {
      const bal = parseFloat(a.balance ?? "0")
      byType.set(a.type, (byType.get(a.type) ?? 0) + bal)
    }

    // By institution
    const byInst = new Map<string, number>()
    for (const a of accounts) {
      const bal = parseFloat(a.balance ?? "0")
      byInst.set(a.institution, (byInst.get(a.institution) ?? 0) + bal)
    }

    // By asset class - mirrors the backend's account_type_to_asset_class
    const byAssetClass = new Map<string, number>()
    for (const a of accounts) {
      const bal = parseFloat(a.balance ?? "0")
      let cls: string
      if (a.type === "investment" || a.type === "investment_isa") cls = "Stocks"
      else if (a.type === "pension") cls = "Pension"
      else if (a.type === "property") cls = "Property"
      else if (a.type === "credit") cls = "Credit"
      else cls = "Cash"
      // Breakdowns use absolute values (matches backend logic) so liabilities
      // show positive for charting.
      byAssetClass.set(cls, (byAssetClass.get(cls) ?? 0) + Math.abs(bal))
    }

    function toBreakdown(map: Map<string, number>) {
      const total = Array.from(map.values()).reduce((s, v) => s + v, 0)
      return Array.from(map.entries())
        .map(([label, val]) => ({
          label,
          value: val.toFixed(2),
          percentage: total > 0 ? (val / total) * 100 : 0,
          display_currency: null,
        }))
        .sort((a, b) => parseFloat(b.value) - parseFloat(a.value))
    }

    // Rough investment metrics for mock mode: sum investment account balances
    // as `end_value` and set the others to zero. The real backend computes
    // these from snapshot deltas + Finance: Investment Transfer outflows.
    const investEndValue = accounts
      .filter((a) => a.type === "investment")
      .reduce((s, a) => s + parseFloat(a.balance ?? "0"), 0)

    return {
      net_worth: netWorth.toFixed(2),
      preferred_currency: "GBP",
      as_of: "2026-03-20",
      total_assets: totalAssets.toFixed(2),
      total_liabilities: totalLiabilities.toFixed(2),
      available_wealth: availableWealth.toFixed(2),
      unavailable_wealth: unavailableWealth.toFixed(2),
      accounts,
      by_type: toBreakdown(byType),
      by_institution: toBreakdown(byInst),
      by_asset_class: toBreakdown(byAssetClass),
      investment_metrics: {
        start_value: "0",
        end_value: investEndValue.toFixed(2),
        total_growth: "0",
        new_cash_invested: "0",
        market_growth: "0",
      },
    }
  }

  async getPortfolioHistory(
    start: string,
    end: string,
    _granularity?: Granularity,
    _profileId?: string
  ): Promise<PortfolioHistoryRow[]> {
    await delay(DELAY_MS)

    // Aggregate snapshots by month, split by available/unavailable
    const months = new Map<
      string,
      { available: number; unavailable: number }
    >()

    for (const snap of MOCK_ACCOUNT_BALANCES) {
      const month = getMonthFromDate(snap.as_of)
      if (start && month < start.substring(0, 7)) continue
      if (end && month > end.substring(0, 7)) continue

      const account = MOCK_ACCOUNTS.find((a) => a.id === snap.account_id)
      if (!account) continue

      if (!months.has(month)) months.set(month, { available: 0, unavailable: 0 })
      const entry = months.get(month)!
      const bal = parseFloat(snap.balance)

      const split = classifyBalance(account.type, bal)
      entry.available += split.available
      entry.unavailable += split.unavailable
    }

    return Array.from(months.entries())
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([month, { available, unavailable }]) => ({
        month,
        available_wealth: available.toFixed(2),
        available_wealth_display: null,
        unavailable_wealth: unavailable.toFixed(2),
        unavailable_wealth_display: null,
        total_wealth: (available + unavailable).toFixed(2),
        total_wealth_display: null,
      }))
  }

  async getHoldings(accountId: string): Promise<Holding[]> {
    await delay(DELAY_MS)
    return MOCK_HOLDINGS.filter((h) => h.account_id === accountId)
  }

  async getHoldingsBatch(accountIds: string[]): Promise<Holding[]> {
    await delay(DELAY_MS)
    const set = new Set(accountIds)
    return MOCK_HOLDINGS.filter((h) => set.has(h.account_id))
  }

  async getCashFlow(
    start: string,
    end: string,
    _granularity?: Granularity,
    _profileId?: string
  ): Promise<CashFlowMonth[]> {
    await delay(DELAY_MS)

    const months = new Map<string, { income: number; spending: number }>()

    for (const t of MOCK_TRANSACTIONS) {
      if (start && t.date < start) continue
      if (end && t.date > end) continue

      const month = getMonthFromDate(t.date)
      if (!months.has(month)) months.set(month, { income: 0, spending: 0 })
      const entry = months.get(month)!
      const amt = parseFloat(t.amount)

      if (amt > 0) {
        entry.income += amt
      } else {
        entry.spending += Math.abs(amt)
      }
    }

    return Array.from(months.entries())
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([month, { income, spending }]) => ({
        month,
        income: income.toFixed(2),
        income_display: null,
        spending: spending.toFixed(2),
        spending_display: null,
      }))
  }

  async getAccountBalances(
    start: string,
    end: string,
    _profileId?: string
  ): Promise<AccountSnapshot[]> {
    await delay(DELAY_MS)
    return MOCK_ACCOUNT_BALANCES.filter((s) => {
      const month = getMonthFromDate(s.as_of)
      if (start && month < start.substring(0, 7)) return false
      if (end && month > end.substring(0, 7)) return false
      return true
    })
  }

  async exportData(format: string): Promise<void> {
    await delay(DELAY_MS)
    console.log(`[Mock] Export requested: format=${format}`)
  }

  // ── Settings / CRUD ──────────────────────────────────────────────

  async createProfile(body: { id: string; name: string }): Promise<Profile> {
    await delay(DELAY_MS)
    const profile: Profile = { id: body.id, name: body.name }
    MOCK_PROFILES.push(profile)
    return profile
  }

  async updateProfile(id: string, body: { name?: string }): Promise<Profile> {
    await delay(DELAY_MS)
    const idx = MOCK_PROFILES.findIndex((p) => p.id === id)
    if (idx === -1) throw new Error(`profile ${id} not found`)
    const updated: Profile = { ...MOCK_PROFILES[idx], ...(body.name ? { name: body.name } : {}) }
    MOCK_PROFILES[idx] = updated
    return updated
  }

  async deleteProfile(id: string): Promise<void> {
    await delay(DELAY_MS)
    if (id === "default") throw new Error("cannot delete the default profile")
    const refs = MOCK_ACCOUNTS.filter((a) => a.profile_ids?.includes(id)).length
    if (refs > 0) throw new Error(`${refs} account(s) still reference profile ${id}`)
    const idx = MOCK_PROFILES.findIndex((p) => p.id === id)
    if (idx === -1) throw new Error(`profile ${id} not found`)
    MOCK_PROFILES.splice(idx, 1)
  }

  async updateAccount(id: string, body: PatchAccountBody): Promise<Account> {
    await delay(DELAY_MS)
    const idx = MOCK_ACCOUNTS.findIndex((a) => a.id === id)
    if (idx === -1) throw new Error(`account ${id} not found`)
    const cur = MOCK_ACCOUNTS[idx]
    const updated: Account = {
      ...cur,
      name: body.name ?? cur.name,
      institution: body.institution ?? cur.institution,
      type: (body.type as Account["type"]) ?? cur.type,
      currency: body.currency ?? cur.currency,
      is_active: body.is_active ?? cur.is_active,
      profile_ids: body.profile_ids ?? cur.profile_ids,
      notes: body.notes !== undefined ? body.notes : cur.notes,
    }
    MOCK_ACCOUNTS[idx] = updated
    return updated
  }

  async deleteAccount(id: string): Promise<void> {
    await delay(DELAY_MS)
    const idx = MOCK_ACCOUNTS.findIndex((a) => a.id === id)
    if (idx === -1) throw new Error(`account ${id} not found`)
    MOCK_ACCOUNTS[idx] = { ...MOCK_ACCOUNTS[idx], is_active: false }
  }

  async createAccount(body: CreateAccountBody): Promise<Account> {
    await delay(DELAY_MS)
    const account: Account = {
      id: body.id,
      name: body.name,
      institution: body.institution,
      type: body.type as Account["type"],
      currency: body.currency ?? "GBP",
      balance: null,
      balance_date: null,
      is_active: true,
      notes: body.notes ?? null,
      profile_ids: body.profile_ids ?? ["default"],
      is_stale: null,
      is_available: true,
    }
    MOCK_ACCOUNTS.push(account)
    return account
  }

  private mockCategoryTree: CategoryNode[] = [
    { id: "food", name: "Food", children: [
      { id: "groceries", name: "Groceries", children: [] },
      { id: "dining", name: "Dining & Bars", children: [] },
    ]},
    { id: "housing", name: "Housing", children: [
      { id: "rent", name: "Rent", children: [] },
      { id: "utilities", name: "Utilities", children: [] },
    ]},
    { id: "transport", name: "Transport", children: [
      { id: "transport-general", name: "Transport", children: [] },
    ]},
    { id: "lifestyle", name: "Lifestyle", children: [
      { id: "entertainment", name: "Entertainment", children: [] },
      { id: "shopping", name: "Shopping", children: [] },
    ]},
    { id: "health", name: "Health", children: [
      { id: "health-general", name: "Health", children: [] },
    ]},
    { id: "income", name: "Income", children: [
      { id: "salary", name: "Salary", children: [] },
    ]},
    { id: "transfers", name: "Transfers", children: [
      { id: "transfers-general", name: "Transfers", children: [] },
    ]},
  ]

  async getCategoryDetails(): Promise<CategoryNode[]> {
    await delay(DELAY_MS)
    return JSON.parse(JSON.stringify(this.mockCategoryTree))
  }

  async createCategory(body: CreateCategoryBody): Promise<Category> {
    await delay(DELAY_MS)
    const now = new Date().toISOString()
    const cat: Category = {
      id: body.name.toLowerCase().replace(/\s+/g, "-"),
      name: body.name,
      parent_id: body.parent_id ?? null,
      display_order: body.display_order ?? 0,
      is_active: true,
      created_at: now,
      updated_at: now,
    }
    if (!cat.parent_id) {
      this.mockCategoryTree.push({ id: cat.id, name: cat.name, children: [] })
    } else {
      const parent = this.mockCategoryTree.find(p => p.id === cat.parent_id)
      if (parent) parent.children.push({ id: cat.id, name: cat.name, children: [] })
    }
    return cat
  }

  async updateCategory(id: string, body: PatchCategoryBody): Promise<Category> {
    await delay(DELAY_MS)
    const now = new Date().toISOString()
    for (const node of this.mockCategoryTree) {
      if (node.id === id) {
        if (body.name) node.name = body.name
        return { id, name: node.name, parent_id: null, display_order: 0, is_active: true, created_at: now, updated_at: now }
      }
      for (const child of node.children) {
        if (child.id === id) {
          if (body.name) child.name = body.name
          return { id, name: child.name, parent_id: node.id, display_order: 0, is_active: true, created_at: now, updated_at: now }
        }
      }
    }
    throw new Error(`Category ${id} not found`)
  }

  async deleteCategory(id: string): Promise<void> {
    await delay(DELAY_MS)
    const topIdx = this.mockCategoryTree.findIndex(n => n.id === id)
    if (topIdx !== -1) { this.mockCategoryTree.splice(topIdx, 1); return }
    for (const node of this.mockCategoryTree) {
      const childIdx = node.children.findIndex(c => c.id === id)
      if (childIdx !== -1) { node.children.splice(childIdx, 1); return }
    }
  }

  async patchTransaction(id: string, body: PatchTransactionBody): Promise<Transaction> {
    await delay(DELAY_MS)
    const tx = MOCK_TRANSACTIONS.find(t => t.id === id)
    if (!tx) throw new Error(`Transaction ${id} not found`)
    if (body.exclude_from_summary !== undefined) tx.exclude_from_summary = body.exclude_from_summary
    if (body.notes !== undefined) tx.notes = body.notes
    if (body.category_id !== undefined) tx.category_id = body.category_id
    return { ...tx }
  }

  // ── Currencies ────────────────────────────────────────────────────

  async getCurrencies(): Promise<Currency[]> {
    await delay(DELAY_MS)
    return [...mockCurrencies]
  }

  async createCurrency(body: { code: string; fx_rate: string }): Promise<Currency> {
    await delay(DELAY_MS)
    const currency: Currency = { code: body.code, is_preferred: false, fx_rate: body.fx_rate, updated_at: new Date().toISOString() }
    mockCurrencies.push(currency)
    return currency
  }

  async updateCurrency(code: string, body: { fx_rate?: string; is_preferred?: boolean }): Promise<Currency> {
    await delay(DELAY_MS)
    const idx = mockCurrencies.findIndex(c => c.code === code)
    if (idx === -1) throw new Error(`Currency ${code} not found`)
    if (body.is_preferred) {
      for (const c of mockCurrencies) c.is_preferred = false
    }
    if (body.fx_rate !== undefined) mockCurrencies[idx].fx_rate = body.fx_rate
    if (body.is_preferred !== undefined) mockCurrencies[idx].is_preferred = body.is_preferred
    if (body.fx_rate !== undefined) mockCurrencies[idx].updated_at = new Date().toISOString()
    return { ...mockCurrencies[idx] }
  }

  async deleteCurrency(code: string): Promise<void> {
    await delay(DELAY_MS)
    const idx = mockCurrencies.findIndex(c => c.code === code)
    if (idx === -1) throw new Error(`Currency ${code} not found`)
    mockCurrencies.splice(idx, 1)
  }

  // ── Import ────────────────────────────────────────────────────────

  async importCsv(accountId: string, file: File): Promise<ImportResult> {
    await delay(DELAY_MS * 2)
    return {
      rows_total: BigInt(42),
      rows_inserted: BigInt(38),
      rows_duplicate: BigInt(4),
      filename: file.name,
      account_id: accountId,
      detected_bank: "monzo",
      detection_confidence: 0.95,
      errors: [],
    }
  }

  async parseDocuments(
    files: File[],
    accountId: string,
    hints: ParseHints
  ): Promise<IngestionPreview> {
    await delay(DELAY_MS * 2)
    const wantTx = hints.return_type.transactions
    const wantHoldings = hints.return_type.holdings.enabled
    const wantInv = hints.return_type.investments

    const txRows = wantTx
      ? [
          { index: 0, date: "2026-05-15T00:00:00", description: "TfL", amount: "-2.80", currency: "GBP", status: "new" as const, existing_id: null, existing_description: null, error_reason: null },
          { index: 1, date: "2026-05-15T00:00:00", description: "Lidl", amount: "-23.45", currency: "GBP", status: "duplicate" as const, existing_id: "tx_abc123", existing_description: "Lidl", error_reason: null },
          { index: 2, date: "2026-05-16T00:00:00", description: "Pret a Manger", amount: "-4.50", currency: "GBP", status: "new" as const, existing_id: null, existing_description: null, error_reason: null },
          { index: 3, date: "2026-05-17T00:00:00", description: "Spotify", amount: "-9.99", currency: "GBP", status: "new" as const, existing_id: null, existing_description: null, error_reason: null },
        ]
      : []
    const txPayload: ImportPayload | null = wantTx
      ? {
          account_id: accountId,
          transactions: [
            { date: "2026-05-15T00:00:00", description: "TfL", amount: "-2.80", currency: "GBP", category: "Transport", category_id: null, category_source: "rule", notes: null, is_recurring: null, exclude_from_summary: null },
            { date: "2026-05-16T00:00:00", description: "Pret a Manger", amount: "-4.50", currency: "GBP", category: "Eating Out", category_id: null, category_source: "rule", notes: null, is_recurring: null, exclude_from_summary: null },
            { date: "2026-05-17T00:00:00", description: "Spotify", amount: "-9.99", currency: "GBP", category: "Subscriptions", category_id: null, category_source: "rule", notes: null, is_recurring: true, exclude_from_summary: null },
          ],
        }
      : null

    const holdingRows = wantHoldings
      ? [
          { account_id: accountId, symbol: "VUSA", sub_account: null, value: "3816.00", currency: "GBP", as_of: "2026-05-17T00:00:00", status: "modify", existing_value: "3654.00" },
          { account_id: accountId, symbol: "AAPL", sub_account: null, value: "1984.50", currency: "USD", as_of: "2026-05-17T00:00:00", status: "new", existing_value: null },
        ]
      : []
    const holdingsPayload: HoldingsImportPayload | null = wantHoldings
      ? {
          account_id: accountId,
          holdings: [
            { account_id: accountId, symbol: "VUSA", name: "Vanguard S&P 500 UCITS ETF", holding_type: "etf", quantity: "50.0000", price_per_unit: "76.32", value: "3816.00", currency: "GBP", as_of: "2026-05-17T00:00:00", short_name: "VUSA", sub_account: null, is_closed: false },
            { account_id: accountId, symbol: "AAPL", name: "Apple Inc", holding_type: "stock", quantity: "10.0000", price_per_unit: "198.45", value: "1984.50", currency: "USD", as_of: "2026-05-17T00:00:00", short_name: "AAPL", sub_account: null, is_closed: false },
          ],
        }
      : null

    const invRows = wantInv
      ? [
          { index: 0, event_type: "buy", symbol: "AAPL", date: "2026-04-10T14:30:00", quantity: "10.0000", price_per_share: "185.20", currency: "USD", status: "new" as const, existing_id: null },
          { index: 1, event_type: "buy", symbol: "VUSA", date: "2026-01-15T09:00:00", quantity: "5.0000", price_per_share: "72.10", currency: "GBP", status: "duplicate" as const, existing_id: "inv_xyz789" },
        ]
      : []
    const invPayload: InvestmentsImportPayload | null = wantInv
      ? {
          account_id: accountId,
          events: [
            { account_id: accountId, event_type: "buy", symbol: "AAPL", date: "2026-04-10T14:30:00", quantity: "10.0000", price_per_share: "185.20", fee: "0.00", currency: "USD", notes: null },
          ],
        }
      : null

    const newCount = (n: number, d = 0) => n - d
    const _ = files // referenced to keep ts-rs happy
    void _
    return {
      status: "success",
      metadata: {
        files_processed: files.length,
        institution_detected: "monzo",
        detection_confidence: 0.97,
        processing_time_ms: BigInt(2340),
        notes: [],
        relationships_found: [],
      },
      transactions: {
        count: txRows.length,
        new: newCount(txRows.filter((r) => r.status === "new").length),
        duplicate: txRows.filter((r) => r.status === "duplicate").length,
        errors: 0,
        rows: txRows,
        payload: txPayload,
      },
      holdings: {
        count: holdingRows.length,
        new: holdingRows.filter((r) => r.status === "new").length,
        modify: holdingRows.filter((r) => r.status === "modify").length,
        rows: holdingRows,
        payload: holdingsPayload,
      },
      investments: {
        count: invRows.length,
        new: invRows.filter((r) => r.status === "new").length,
        duplicate: invRows.filter((r) => r.status === "duplicate").length,
        rows: invRows,
        payload: invPayload,
      },
      clarifications_needed: [],
    }
  }

  async commitTransactions(payload: ImportPayload): Promise<ImportResult> {
    await delay(DELAY_MS)
    return {
      rows_total: BigInt(payload.transactions.length),
      rows_inserted: BigInt(payload.transactions.length),
      rows_duplicate: BigInt(0),
      filename: "<api>",
      account_id: payload.account_id,
      detected_bank: "unknown",
      detection_confidence: 0,
      errors: [],
    }
  }

  async commitHoldings(payload: HoldingsImportPayload): Promise<HoldingsImportResponse> {
    await delay(DELAY_MS)
    return { inserted: payload.holdings.length, updated: 0, total: payload.holdings.length }
  }

  async commitInvestments(payload: InvestmentsImportPayload): Promise<InvestmentImportResult> {
    await delay(DELAY_MS)
    return { total: payload.events.length, inserted: payload.events.length, duplicates: 0, errors: [] }
  }
}
