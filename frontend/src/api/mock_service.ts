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
import type { CategorySource } from "@/bindings/CategorySource"
import type { ImportPayload } from "@/bindings/ImportPayload"
import type { HoldingsImportPayload } from "@/bindings/HoldingsImportPayload"
import type { InvestmentsImportPayload } from "@/bindings/InvestmentsImportPayload"
import type { InvestmentImportResult } from "@/bindings/InvestmentImportResult"
import type { CapitalGainsResponse } from "@/bindings/CapitalGainsResponse"
import type { CgtRealizedEvent } from "@/bindings/CgtRealizedEvent"
import type { S104PoolState } from "@/bindings/S104PoolState"
import type { SymbolSummary } from "@/bindings/SymbolSummary"
import type { DocumentSummary } from "@/bindings/DocumentSummary"
import type { DocumentDeleteResult } from "@/bindings/DocumentDeleteResult"
import type { ApiService, CgtFilters, HoldingsImportResponse } from "./service"
import { DocumentReferencedError } from "./service"
import { cgtFiltersToParams } from "./cgt_filter_params"
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
  // In-memory document store so the Documents page renders in mock mode. One
  // referenced (orphaned = false) and one orphaned entry to exercise the badge.
  private documents: DocumentSummary[] = [
    {
      id: "mock_doc_monzo_may",
      filename: "monzo_may_2026.csv",
      mime_type: "text/csv",
      size_bytes: 18234,
      origin: "parse",
      account_id: "monzo-alex",
      uploaded_at: "2026-05-31T09:14:02Z",
      reference_count: 47,
      orphaned: false,
    },
    {
      id: "mock_doc_orphan",
      filename: "t212_positions_draft.csv",
      mime_type: "text/csv",
      size_bytes: 4096,
      origin: "parse",
      account_id: "t212-isa",
      uploaded_at: "2026-06-02T18:40:00Z",
      reference_count: 0,
      orphaned: true,
    },
  ]

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
      const wantUncategorized = set.delete("__uncategorized__")
      data = data.filter((t) => {
        if (t.category == null || t.category === "") return wantUncategorized
        return set.has(t.category)
      })
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

    // Sort BEFORE pagination so it matches what the backend would return.
    const sort = filters.sort
    const dir = filters.sort_dir ?? "desc"
    const sign = dir === "asc" ? 1 : -1
    if (sort) {
      data = [...data].sort((a, b) => {
        let av: string | number
        let bv: string | number
        switch (sort) {
          case "date":
            av = a.date
            bv = b.date
            break
          case "amount":
            av = parseFloat(a.amount)
            bv = parseFloat(b.amount)
            break
          case "category": {
            const aHas = a.category != null && a.category !== ""
            const bHas = b.category != null && b.category !== ""
            // Uncategorized always at the bottom regardless of direction.
            if (aHas !== bHas) return aHas ? -1 : 1
            av = a.category ?? ""
            bv = b.category ?? ""
            break
          }
        }
        if (av < bv) return -1 * sign
        if (av > bv) return 1 * sign
        return a.id < b.id ? -1 : a.id > b.id ? 1 : 0
      })
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

  async getCategoriesWithIds(): Promise<Array<{ id: string; name: string }>> {
    await delay(DELAY_MS)
    const cats = new Set<string>()
    for (const t of MOCK_TRANSACTIONS) {
      if (t.category) cats.add(t.category)
    }
    const fromMock = Array.from(cats)
      .sort()
      .map((name) => ({
        id: `cat-${name.toLowerCase().replace(/[^a-z0-9]+/g, "-")}`,
        name,
      }))
    // Demo ids returned by mock `parseDocuments` so the preview can resolve
    // them to readable names in the SelectCell.
    return [
      ...fromMock,
      { id: "cat-transport", name: "Transport" },
      { id: "cat-groceries", name: "Groceries" },
      { id: "cat-eating-out", name: "Eating Out" },
      { id: "cat-subscriptions", name: "Subscriptions" },
    ]
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
    _profileId?: string,
    excludeCategoryIds?: string[]
  ): Promise<CashFlowMonth[]> {
    await delay(DELAY_MS)

    const months = new Map<string, { income: number; spending: number }>()
    const excludeSet = new Set(excludeCategoryIds ?? [])

    for (const t of MOCK_TRANSACTIONS) {
      if (start && t.date < start) continue
      if (end && t.date > end) continue
      if (t.category_id && excludeSet.has(t.category_id)) continue

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
    { id: "food", name: "Food", description: null, children: [
      { id: "groceries", name: "Groceries", description: null, children: [] },
      { id: "dining", name: "Dining & Bars", description: null, children: [] },
    ]},
    { id: "housing", name: "Housing", description: null, children: [
      { id: "rent", name: "Rent", description: null, children: [] },
      { id: "utilities", name: "Utilities", description: null, children: [] },
    ]},
    { id: "transport", name: "Transport", description: null, children: [
      { id: "transport-general", name: "Transport", description: null, children: [] },
    ]},
    { id: "lifestyle", name: "Lifestyle", description: null, children: [
      { id: "entertainment", name: "Entertainment", description: null, children: [] },
      { id: "shopping", name: "Shopping", description: null, children: [] },
    ]},
    { id: "health", name: "Health", description: null, children: [
      { id: "health-general", name: "Health", description: null, children: [] },
    ]},
    { id: "income", name: "Income", description: null, children: [
      { id: "salary", name: "Salary", description: null, children: [] },
    ]},
    { id: "transfers", name: "Transfers", description: null, children: [
      { id: "transfers-general", name: "Transfers", description: null, children: [] },
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
      description: body.description ?? null,
      created_at: now,
      updated_at: now,
    }
    if (!cat.parent_id) {
      this.mockCategoryTree.push({ id: cat.id, name: cat.name, description: cat.description, children: [] })
    } else {
      const parent = this.mockCategoryTree.find(p => p.id === cat.parent_id)
      if (parent) parent.children.push({ id: cat.id, name: cat.name, description: null, children: [] })
    }
    return cat
  }

  async updateCategory(id: string, body: PatchCategoryBody): Promise<Category> {
    await delay(DELAY_MS)
    const now = new Date().toISOString()
    for (const node of this.mockCategoryTree) {
      if (node.id === id) {
        if (body.name) node.name = body.name
        return { id, name: node.name, parent_id: null, display_order: 0, is_active: true, description: null, created_at: now, updated_at: now }
      }
      for (const child of node.children) {
        if (child.id === id) {
          if (body.name) child.name = body.name
          return { id, name: child.name, parent_id: node.id, display_order: 0, is_active: true, description: null, created_at: now, updated_at: now }
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

    const isUnified = hints.experimental?.mode === "unified"
    const txRows = wantTx
      ? [
          { index: 0, date: "2026-05-15T00:00:00", description: "TfL", amount: "-2.80", currency: "GBP", status: "new" as const, existing_id: null, existing_description: null, error_reason: null, category_id: isUnified ? "cat-transport" : null, category_confidence: isUnified ? 0.95 : null, source_document_ids: [] },
          { index: 1, date: "2026-05-15T00:00:00", description: "Lidl", amount: "-23.45", currency: "GBP", status: "duplicate" as const, existing_id: "tx_abc123", existing_description: "Lidl", error_reason: null, category_id: isUnified ? "cat-groceries" : null, category_confidence: isUnified ? 0.97 : null, source_document_ids: [] },
          { index: 2, date: "2026-05-16T00:00:00", description: "Pret a Manger", amount: "-4.50", currency: "GBP", status: "new" as const, existing_id: null, existing_description: null, error_reason: null, category_id: isUnified ? "cat-eating-out" : null, category_confidence: isUnified ? 0.78 : null, source_document_ids: [] },
          { index: 3, date: "2026-05-17T00:00:00", description: "Spotify", amount: "-9.99", currency: "GBP", status: "new" as const, existing_id: null, existing_description: null, error_reason: null, category_id: isUnified ? "cat-subscriptions" : null, category_confidence: isUnified ? 0.45 : null, source_document_ids: [] },
        ]
      : []
    const txPayload: ImportPayload | null = wantTx
      ? {
          account_id: accountId,
          transactions: [
            { date: "2026-05-15T00:00:00", description: "TfL", amount: "-2.80", currency: "GBP", category: null, category_id: isUnified ? "cat-transport" : null, category_source: isUnified ? ("agent" satisfies CategorySource) : null, notes: null, is_recurring: null, exclude_from_summary: null, source_document_ids: [] },
            { date: "2026-05-16T00:00:00", description: "Pret a Manger", amount: "-4.50", currency: "GBP", category: null, category_id: isUnified ? "cat-eating-out" : null, category_source: isUnified ? ("agent" satisfies CategorySource) : null, notes: null, is_recurring: null, exclude_from_summary: null, source_document_ids: [] },
            { date: "2026-05-17T00:00:00", description: "Spotify", amount: "-9.99", currency: "GBP", category: null, category_id: isUnified ? "cat-subscriptions" : null, category_source: isUnified ? ("agent" satisfies CategorySource) : null, notes: null, is_recurring: true, exclude_from_summary: null, source_document_ids: [] },
          ],
        }
      : null

    const holdingRows = wantHoldings
      ? [
          { account_id: accountId, symbol: "VUSA", sub_account: null, value: "3816.00", currency: "GBP", as_of: "2026-05-17T00:00:00", status: "modify", existing_value: "3654.00", derived: false, source_document_ids: [] },
          { account_id: accountId, symbol: "AAPL", sub_account: null, value: "1984.50", currency: "USD", as_of: "2026-05-17T00:00:00", status: "new", existing_value: null, derived: true, source_document_ids: [] },
        ]
      : []
    const holdingsPayload: HoldingsImportPayload | null = wantHoldings
      ? {
          account_id: accountId,
          holdings: [
            { account_id: accountId, symbol: "VUSA", name: "Vanguard S&P 500 UCITS ETF", holding_type: "etf", quantity: "50.0000", price_per_unit: "76.32", value: "3816.00", currency: "GBP", as_of: "2026-05-17T00:00:00", short_name: "VUSA", sub_account: null, is_closed: false, derived: false, source_document_ids: [] },
            { account_id: accountId, symbol: "AAPL", name: "Apple Inc", holding_type: "stock", quantity: "10.0000", price_per_unit: "198.45", value: "1984.50", currency: "USD", as_of: "2026-05-17T00:00:00", short_name: "AAPL", sub_account: null, is_closed: false, derived: true, source_document_ids: [] },
          ],
        }
      : null

    const invRows = wantInv
      ? [
          { index: 0, event_type: "buy", symbol: "AAPL", date: "2026-04-10T14:30:00", quantity: "10.0000", price_per_share: "185.20", currency: "USD", status: "new" as const, existing_id: null, source_document_ids: [] },
          { index: 1, event_type: "buy", symbol: "VUSA", date: "2026-01-15T09:00:00", quantity: "5.0000", price_per_share: "72.10", currency: "GBP", status: "duplicate" as const, existing_id: "inv_xyz789", source_document_ids: [] },
        ]
      : []
    const invPayload: InvestmentsImportPayload | null = wantInv
      ? {
          account_id: accountId,
          events: [
            { account_id: accountId, event_type: "buy", symbol: "AAPL", date: "2026-04-10T14:30:00", quantity: "10.0000", price_per_share: "185.20", fee: "0.00", currency: "USD", notes: null, source_document_ids: [] },
          ],
        }
      : null

    const newCount = (n: number, d = 0) => n - d
    const _ = files // referenced to keep ts-rs happy
    void _
    const unifiedAgent = (hints.experimental?.agent ?? "sonnet") as "haiku" | "sonnet" | "opus"
    const splitAgent = (hints.experimental?.agent ?? "haiku") as "haiku" | "sonnet" | "opus"
    const modelFor: Record<"haiku" | "sonnet" | "opus", string> = {
      haiku: "claude-haiku-4-5-20251001",
      sonnet: "claude-sonnet-4-6",
      opus: "claude-opus-4-7",
    }
    const calls = isUnified
      ? [
          {
            parser: "unified",
            agent: unifiedAgent,
            model: modelFor[unifiedAgent],
            input_tokens: BigInt(8420),
            output_tokens: BigInt(1180),
            duration_ms: BigInt(2340),
            amount: "0.0430",
            currency: "USD",
          },
        ]
      : [
          ...(wantTx
            ? [
                {
                  parser: "csv_transactions",
                  agent: splitAgent,
                  model: modelFor[splitAgent],
                  input_tokens: BigInt(3120),
                  output_tokens: BigInt(540),
                  duration_ms: BigInt(820),
                  amount: "0.0058",
                  currency: "USD",
                },
              ]
            : []),
          ...(wantHoldings
            ? [
                {
                  parser: "csv_holdings",
                  agent: splitAgent,
                  model: modelFor[splitAgent],
                  input_tokens: BigInt(2100),
                  output_tokens: BigInt(340),
                  duration_ms: BigInt(710),
                  amount: "0.0038",
                  currency: "USD",
                },
              ]
            : []),
          ...(wantInv
            ? [
                {
                  parser: "csv_investments",
                  agent: splitAgent,
                  model: modelFor[splitAgent],
                  input_tokens: BigInt(1840),
                  output_tokens: BigInt(420),
                  duration_ms: BigInt(620),
                  amount: "0.0039",
                  currency: "USD",
                },
              ]
            : []),
        ]
    const totalUsd = calls
      .reduce((s, c) => s + parseFloat(c.amount), 0)
      .toFixed(4)

    return {
      status: "success",
      documents: [],
      metadata: {
        files_processed: files.length,
        institution_detected: "monzo",
        detection_confidence: 0.97,
        processing_time_ms: BigInt(2340),
        notes: [],
        relationships_found: [],
        estimated_price: {
          calls,
          total: totalUsd,
          currency: "USD",
        },
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
        known_holdings: wantHoldings
          ? [
              { symbol: "VUSA", name: "Vanguard S&P 500 UCITS ETF", holding_type: "etf", currency: "GBP", sub_account: null, last_value: "3654.00", last_as_of: "2026-02-29" },
            ]
          : [],
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

  // ── Reports ───────────────────────────────────────────────────────

  async getCapitalGains(filters: CgtFilters): Promise<CapitalGainsResponse> {
    await delay(DELAY_MS)
    return mockCapitalGains(filters)
  }

  // ── Documents ─────────────────────────────────────────────────────

  async listDocuments(): Promise<DocumentSummary[]> {
    await delay(DELAY_MS)
    return [...this.documents]
  }

  async uploadDocuments(files: File[], accountId?: string): Promise<DocumentSummary[]> {
    await delay(DELAY_MS)
    const created = files.map((f) => {
      const doc: DocumentSummary = {
        id: `mock_doc_${Math.random().toString(36).slice(2, 10)}`,
        filename: f.name,
        mime_type: f.type || "application/octet-stream",
        size_bytes: f.size,
        origin: "manual",
        account_id: accountId ?? null,
        uploaded_at: new Date().toISOString(),
        reference_count: 0,
        orphaned: true,
      }
      this.documents.unshift(doc)
      return doc
    })
    return created
  }

  async deleteDocument(id: string, force = false): Promise<DocumentDeleteResult> {
    await delay(DELAY_MS)
    const doc = this.documents.find((d) => d.id === id)
    if (doc && doc.reference_count > 0 && !force) {
      // Synthesize a plausible breakdown for the confirm dialog.
      throw new DocumentReferencedError({
        transactions: doc.reference_count,
        holdings: 0,
        investments: 0,
      })
    }
    this.documents = this.documents.filter((d) => d.id !== id)
    return {
      deleted: true,
      unlinked: { transactions: doc?.reference_count ?? 0, holdings: 0, investments: 0 },
    }
  }

  documentDownloadUrl(id: string): string {
    return `/api/documents/${encodeURIComponent(id)}/download`
  }
}

// ── Mock CGT data ─────────────────────────────────────────────────────────────
//
// Seed enough realized events to exercise the UI's edge-case styling without
// reimplementing the HMRC engine here. Each event is stored already-converted
// into GBP (the preferred currency) to match the real route's output shape.

const MOCK_REALIZED_EVENTS: CgtRealizedEvent[] = [
  // VUSA (GBP) — clean S104 disposal in 2024-25
  {
    symbol: "VUSA",
    disposal_id: "mock_disp_vusa_1",
    disposal_date: "2024-09-15T10:00:00",
    quantity: "30",
    disposal_price: "85.50",
    proceeds: "2565.00",
    cost_basis: "2160.00",
    gain_loss: "405.00",
    rule_applied: "S104 Pool",
    original_currency: "GBP",
    matches: [
      { acquisition_id: null, acquisition_date: "S104 Pool", quantity: "30", price: "72.00" },
    ],
  },
  // AAPL (USD) — same-day match in 2024-25
  {
    symbol: "AAPL",
    disposal_id: "mock_disp_aapl_1",
    disposal_date: "2024-11-02T14:30:00",
    quantity: "20",
    disposal_price: "182.40",
    proceeds: "2881.92",
    cost_basis: "2528.00",
    gain_loss: "353.92",
    rule_applied: "Same-Day",
    original_currency: "USD",
    matches: [
      {
        acquisition_id: "mock_acq_aapl_sameday",
        acquisition_date: "2024-11-02T09:00:00",
        quantity: "20",
        price: "160.00",
      },
    ],
  },
  // AAPL (USD) — S104 disposal at a loss in 2024-25
  {
    symbol: "AAPL",
    disposal_id: "mock_disp_aapl_2",
    disposal_date: "2025-01-29T15:30:00",
    quantity: "10",
    disposal_price: "150.00",
    proceeds: "1185.00",
    cost_basis: "1264.00",
    gain_loss: "-79.00",
    rule_applied: "S104 Pool",
    original_currency: "USD",
    matches: [
      { acquisition_id: null, acquisition_date: "S104 Pool", quantity: "10", price: "126.40" },
    ],
  },
  // VUSA (GBP) — 30-day Bed & Breakfast match in 2025-26
  {
    symbol: "VUSA",
    disposal_id: "mock_disp_vusa_2",
    disposal_date: "2025-05-12T10:00:00",
    quantity: "15",
    disposal_price: "88.00",
    proceeds: "1320.00",
    cost_basis: "1335.00",
    gain_loss: "-15.00",
    rule_applied: "30-Day Rule",
    original_currency: "GBP",
    matches: [
      {
        acquisition_id: "mock_acq_vusa_30day",
        acquisition_date: "2025-05-25T10:00:00",
        quantity: "15",
        price: "89.00",
      },
    ],
  },
  // AAPL (USD) — Unmatched disposal in 2025-26 (the edge-case styling)
  {
    symbol: "AAPL",
    disposal_id: "mock_disp_aapl_unmatched",
    disposal_date: "2025-07-10T11:00:00",
    quantity: "5",
    disposal_price: "210.00",
    proceeds: "829.50",
    cost_basis: "0",
    gain_loss: "829.50",
    rule_applied: "Unmatched",
    original_currency: "USD",
    matches: [
      { acquisition_id: null, acquisition_date: null, quantity: "5", price: "0" },
    ],
  },
  // VUSA (GBP) — clean S104 disposal in 2025-26
  {
    symbol: "VUSA",
    disposal_id: "mock_disp_vusa_3",
    disposal_date: "2026-02-04T10:00:00",
    quantity: "25",
    disposal_price: "92.00",
    proceeds: "2300.00",
    cost_basis: "1800.00",
    gain_loss: "500.00",
    rule_applied: "S104 Pool",
    original_currency: "GBP",
    matches: [
      { acquisition_id: null, acquisition_date: "S104 Pool", quantity: "25", price: "72.00" },
    ],
  },
]

const MOCK_POOLS: S104PoolState[] = [
  {
    symbol: "VUSA",
    current_shares: "120",
    total_allowable_expenditure: "8640.00",
    average_cost_per_share: "72.00",
  },
  {
    symbol: "AAPL",
    current_shares: "45",
    total_allowable_expenditure: "5688.00",
    average_cost_per_share: "126.40",
  },
]

function mockCapitalGains(filters: CgtFilters): CapitalGainsResponse {
  const params = cgtFiltersToParams(filters)
  const start = params.start_date ?? null
  const end = params.end_date ?? null

  const events = MOCK_REALIZED_EVENTS.filter((e) => {
    const date = e.disposal_date.slice(0, 10)
    if (start && date < start) return false
    if (end && date > end) return false
    return true
  })

  let totalProceeds = 0
  let totalCosts = 0
  let totalGains = 0
  let totalLosses = 0
  const perSymbol: Record<string, SymbolSummary> = {}

  for (const ev of events) {
    const p = Number.parseFloat(ev.proceeds)
    const c = Number.parseFloat(ev.cost_basis)
    const g = Number.parseFloat(ev.gain_loss)
    totalProceeds += p
    totalCosts += c
    if (g > 0) totalGains += g
    else totalLosses += Math.abs(g)
    if (!perSymbol[ev.symbol]) {
      perSymbol[ev.symbol] = {
        symbol: ev.symbol,
        total_proceeds: "0",
        total_allowable_costs: "0",
        total_gains: "0",
        total_losses: "0",
        net_gain_loss: "0",
        original_currency: ev.original_currency,
      }
    }
    const sym = perSymbol[ev.symbol]
    sym.total_proceeds = (Number.parseFloat(sym.total_proceeds) + p).toFixed(2)
    sym.total_allowable_costs = (Number.parseFloat(sym.total_allowable_costs) + c).toFixed(2)
    if (g > 0) {
      sym.total_gains = (Number.parseFloat(sym.total_gains) + g).toFixed(2)
    } else {
      sym.total_losses = (Number.parseFloat(sym.total_losses) + Math.abs(g)).toFixed(2)
    }
  }

  const symbol_summaries = Object.values(perSymbol).map((s) => ({
    ...s,
    net_gain_loss: (
      Number.parseFloat(s.total_gains) - Number.parseFloat(s.total_losses)
    ).toFixed(2),
  }))
  symbol_summaries.sort((a, b) => a.symbol.localeCompare(b.symbol))

  return {
    summary: {
      total_proceeds: totalProceeds.toFixed(2),
      total_allowable_costs: totalCosts.toFixed(2),
      total_gains: totalGains.toFixed(2),
      total_losses: totalLosses.toFixed(2),
      net_gain_loss: (totalGains - totalLosses).toFixed(2),
      base_currency: "GBP",
    },
    symbol_summaries,
    realized_events: events,
    pools: MOCK_POOLS,
  }
}
