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
import type { CategorySource } from "@/bindings/CategorySource"
import type { ImportPayload } from "@/bindings/ImportPayload"
import type { HoldingsImportPayload } from "@/bindings/HoldingsImportPayload"
import type { InvestmentsImportPayload } from "@/bindings/InvestmentsImportPayload"
import type { InvestmentImportResult } from "@/bindings/InvestmentImportResult"
import type { InvestmentEvent } from "@/bindings/InvestmentEvent"
import type { InvestmentHistoryRow } from "@/bindings/InvestmentHistoryRow"
import type { CreateInvestmentEventBody } from "@/bindings/CreateInvestmentEventBody"
import type { PatchInvestmentEventBody } from "@/bindings/PatchInvestmentEventBody"
import type { InvestmentEventType } from "@/bindings/InvestmentEventType"
import type { CapitalGainsResponse } from "@/bindings/CapitalGainsResponse"
import type { CgtDisposalGroup } from "@/bindings/CgtDisposalGroup"
import type { ExchangeRate } from "@/bindings/ExchangeRate"
import type { ExchangeRateInput } from "@/bindings/ExchangeRateInput"
import type { CgtRealizedEvent } from "@/bindings/CgtRealizedEvent"
import type { S104PoolState } from "@/bindings/S104PoolState"
import type { SymbolSummary } from "@/bindings/SymbolSummary"
import type { DocumentSummary } from "@/bindings/DocumentSummary"
import type { DocumentDeleteResult } from "@/bindings/DocumentDeleteResult"
import type { AccountHoldingsHistory, ApiService, CgtFilters, HoldingsImportResponse, ParseOptions } from "./service"
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

/** Date-keyed rates entered in mock mode. Empty at start — rates are user-owned. */
const mockExchangeRates: ExchangeRate[] = []

// Available/unavailable account type classification
const AVAILABLE_TYPES = new Set(["checking", "savings", "emergency_fund", "investment", "cash", "credit"])
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
  // `reference_count` is null in the list (matching the real backend), so the
  // page's lazy per-row count path is exercised; getDocument resolves it.
  private documents: DocumentSummary[] = [
    {
      id: "mock_doc_monzo_may",
      filename: "monzo_may_2026.csv",
      mime_type: "text/csv",
      size_bytes: 18234,
      origin: "parse",
      account_id: "monzo-alex",
      uploaded_at: "2026-05-31T09:14:02Z",
      reference_count: null,
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
      reference_count: null,
      orphaned: true,
    },
  ]

  // Plausible per-document counts resolved lazily via getDocument. Mirrors the
  // backend: the orphaned doc resolves to 0, the linked one to a non-zero count.
  private documentRefCounts: Record<string, number> = {
    mock_doc_monzo_may: 47,
    mock_doc_orphan: 0,
  }

  // In-memory investment-events ledger so the Investments page renders and
  // mutates in mock mode. Mirrors the CGT mock symbols (VUSA, AAPL).
  private investments: InvestmentEvent[] = [
    mockEvent("mock_inv_vusa_buy_1", "t212-isa-alex", "buy", "VUSA", "2024-03-04T09:00:00", "60.0000", "72.00", "1.00", "GBP"),
    mockEvent("mock_inv_vusa_buy_2", "t212-isa-alex", "buy", "VUSA", "2024-08-12T09:30:00", "60.0000", "72.00", "1.00", "GBP"),
    mockEvent("mock_inv_vusa_sell_1", "t212-isa-alex", "sell", "VUSA", "2024-09-15T10:00:00", "30.0000", "85.50", "1.00", "GBP"),
    mockEvent("mock_inv_aapl_buy_1", "t212-isa-sam", "buy", "AAPL", "2024-06-20T14:30:00", "40.0000", "126.40", "0.00", "USD"),
    mockEvent("mock_inv_aapl_vest_1", "t212-isa-sam", "vest", "AAPL", "2024-10-01T00:00:00", "25.0000", "150.00", null, "USD"),
    mockEvent("mock_inv_aapl_sell_1", "t212-isa-sam", "sell", "AAPL", "2025-01-29T15:30:00", "10.0000", "150.00", "0.50", "USD"),
  ]

  async getProfiles(): Promise<Profile[]> {
    await delay(DELAY_MS)
    return MOCK_PROFILES
  }

  async getTransactions(
    filters: TransactionFilters
  ): Promise<Paginated<Transaction>> {
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
        if (t.category_id == null || t.category_id === "") return wantUncategorized
        return set.has(t.category_id)
      })
    }
    if (filters.search) {
      const q = filters.search.toLowerCase()
      data = data.filter(
        (t) =>
          t.normalized.toLowerCase().includes(q) ||
          t.description.toLowerCase().includes(q) ||
          (t.category_id ?? "").toLowerCase().includes(q) ||
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
            const aHas = a.category_id != null && a.category_id !== ""
            const bHas = b.category_id != null && b.category_id !== ""
            // Uncategorized always at the bottom regardless of direction.
            if (aHas !== bHas) return aHas ? -1 : 1
            av = a.category_id ?? ""
            bv = b.category_id ?? ""
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
      data = data.filter((t) => t.category_id !== null && set.has(t.category_id))
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
      if (!t.category_id) continue
      const amt = parseFloat(t.amount)
      const contribution = filters.direction ? Math.abs(amt) : amt
      totals.set(t.category_id, (totals.get(t.category_id) ?? 0) + contribution)
    }

    // DESC order to match the backend's ORDER BY total DESC
    return Array.from(totals.entries())
      .map(([category_id, total]) => ({ category_id, total: total.toFixed(2), display_currency: null }))
      .sort((a, b) => parseFloat(b.total) - parseFloat(a.total))
  }

  async getCategories(): Promise<string[]> {
    await delay(DELAY_MS)
    const cats = new Set<string>()
    for (const t of MOCK_TRANSACTIONS) {
      if (t.category_id) cats.add(t.category_id)
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
      if (!t.category_id) continue
      spending.set(
        t.category_id,
        (spending.get(t.category_id) ?? 0) + Math.abs(amt)
      )
    }

    return budgets.map((b) => {
      const actual = spending.get(b.category) ?? 0
      const budgeted = parseFloat(b.amount)
      return {
        category_id: b.category,
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
    profileId?: string,
    _filters?: SpendingGridFilters
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

    // Build lookups from the mock category tree: "Parent: Child" -> type, and
    // parent name -> parent id (so grouping aligns with the category context).
    const leafType = new Map<string, string>()
    const parentNameToId = new Map<string, string>()
    for (const parent of this.mockCategoryTree) {
      parentNameToId.set(parent.name, parent.id)
      for (const child of parent.children) {
        leafType.set(`${parent.name}: ${child.name}`, child.category_type)
      }
    }

    const groupBy = _filters?.groupBy ?? "leaf_category"
    const typeFilter = _filters?.categoryTypes && _filters.categoryTypes.length > 0
      ? new Set(_filters.categoryTypes)
      : null

    // Aggregate transactions into the chosen grouping dimension, per month.
    type Cell = { catId: string | null; parentId: string | null; groupKey: string | null; byMonth: Map<string, number> }
    const grid = new Map<string, Cell>()
    for (const t of MOCK_TRANSACTIONS) {
      if (t.date < start || t.date > end) continue
      if (profileAccounts && !profileAccounts.has(t.account_id)) continue
      const cat = t.category_id ?? "Other: Uncategorized"
      const parentName = cat.split(": ")[0].trim()
      const parentId = parentNameToId.get(parentName) ?? parentName
      const ctype = leafType.get(cat) ?? "spending"
      if (typeFilter && !typeFilter.has(ctype)) continue

      let key: string
      let cell: Cell
      switch (groupBy) {
        case "parent_category":
          key = parentId; cell = { catId: null, parentId: null, groupKey: parentId, byMonth: new Map() }; break
        case "category_type":
          key = ctype; cell = { catId: null, parentId: null, groupKey: ctype, byMonth: new Map() }; break
        case "account":
          key = t.account_id; cell = { catId: null, parentId: null, groupKey: t.account_id, byMonth: new Map() }; break
        default:
          key = cat; cell = { catId: cat, parentId, groupKey: null, byMonth: new Map() }
      }
      if (!grid.has(key)) grid.set(key, cell)
      const e = grid.get(key)!
      const month = getMonthFromDate(t.date)
      e.byMonth.set(month, (e.byMonth.get(month) ?? 0) + parseFloat(t.amount))
    }

    const rows: SpendingGridRow[] = []
    for (const [, e] of grid) {
      const monthValues: Record<string, string | null> = {}
      let total = 0
      let monthsWithData = 0
      for (const m of months) {
        if (e.byMonth.has(m)) {
          const val = e.byMonth.get(m)!
          monthValues[m] = val.toFixed(2)
          total += val
          monthsWithData++
        } else {
          monthValues[m] = null
        }
      }
      const avg = monthsWithData > 0 ? total / monthsWithData : 0
      const budget = e.catId ? MOCK_BUDGETS.find((b) => b.category === e.catId) : undefined

      rows.push({
        category_id: e.catId,
        parent_id: e.parentId,
        group_key: e.groupKey,
        periods: monthValues,
        periods_display: {},
        average: avg.toFixed(2),
        average_display: null,
        budget: budget?.amount ?? null,
        total: total.toFixed(2),
        total_display: null,
      })
    }

    rows.sort((a, b) =>
      (a.group_key ?? a.category_id ?? "").localeCompare(b.group_key ?? b.category_id ?? ""),
    )

    return rows
  }

  async getCashSummary(_start: string, _end: string, _profileId?: string): Promise<CashSummaryResponse> {
    await delay(DELAY_MS)
    return {
      preferred_currency: "GBP",
      income: "32000.00",
      spending: "18450.20",
      savings_growth: "5000.00",
      new_cash_invested: "8000.00",
      investment_metrics: {
        start_value: "100000.00",
        end_value: "120000.00",
        total_growth: "20000.00",
        new_cash_invested: "8000.00",
        market_growth: "12000.00",
      },
    }
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

  async getPortfolio(profileId?: string, _asOf?: string): Promise<PortfolioResponse> {
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

  async getAccountHoldingsHistory(
    accountId: string,
    start: string,
    end: string,
    granularity: Granularity = "monthly"
  ): Promise<AccountHoldingsHistory> {
    await delay(DELAY_MS)

    // The mock dataset only stores one snapshot per holding, so synthesize a
    // per-holding series by distributing each period's account balance across
    // the account's current holdings proportionally to their current value.
    const holdings = MOCK_HOLDINGS.filter((h) => h.account_id === accountId)
    const currentTotal = holdings.reduce((s, h) => s + parseFloat(h.value), 0)
    const weights = holdings.map((h) => ({
      symbol: h.symbol,
      weight: currentTotal > 0 ? parseFloat(h.value) / currentTotal : 0,
    }))

    const symbols = holdings.map((h) => ({
      symbol: h.symbol,
      name: h.name,
      short_name: h.short_name,
      holding_type: h.holding_type,
    }))

    // account_id -> "YYYY-MM" -> balance, carried forward across gaps.
    const balanceByMonth = new Map<string, number>()
    for (const snap of MOCK_ACCOUNT_BALANCES) {
      if (snap.account_id !== accountId) continue
      balanceByMonth.set(getMonthFromDate(snap.as_of), parseFloat(snap.balance))
    }

    const periodKey = (month: string): string => {
      if (granularity === "monthly") return month
      const [y, m] = month.split("-").map(Number)
      if (granularity === "quarterly") return `${y}-Q${Math.floor((m - 1) / 3) + 1}`
      return `${y}`
    }

    // Build period-end values: walk months ascending, carry the last known
    // balance forward, and keep the latest month's value per period bucket.
    const rowByPeriod = new Map<string, { period: string; total: string; values: { symbol: string; value: string }[] }>()
    let lastBalance = 0
    for (const month of getMonthsInRange(start, end)) {
      if (balanceByMonth.has(month)) lastBalance = balanceByMonth.get(month)!
      const total = lastBalance
      rowByPeriod.set(periodKey(month), {
        period: periodKey(month),
        total: total.toFixed(2),
        values: weights.map((w) => ({ symbol: w.symbol, value: (total * w.weight).toFixed(2) })),
      })
    }

    return {
      preferred_currency: "GBP",
      symbols,
      rows: Array.from(rowByPeriod.values()),
    }
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

  async getAccountBalances(start: string, end: string): Promise<AccountSnapshot[]> {
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
    const profile: Profile = { id: body.id, name: body.name, utr: null }
    MOCK_PROFILES.push(profile)
    return profile
  }

  async updateProfile(
    id: string,
    body: { name?: string; utr?: string | null },
  ): Promise<Profile> {
    await delay(DELAY_MS)
    const idx = MOCK_PROFILES.findIndex((p) => p.id === id)
    if (idx === -1) throw new Error(`profile ${id} not found`)
    const updated: Profile = {
      ...MOCK_PROFILES[idx],
      ...(body.name ? { name: body.name } : {}),
      // An explicit null clears it; an absent key leaves it alone.
      ...(body.utr !== undefined ? { utr: body.utr } : {}),
    }
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
    { id: "food", name: "Food", description: null, category_type: "spending", children: [
      { id: "groceries", name: "Groceries", description: null, category_type: "spending", children: [] },
      { id: "dining", name: "Dining & Bars", description: null, category_type: "spending", children: [] },
    ]},
    { id: "housing", name: "Housing", description: null, category_type: "spending", children: [
      { id: "rent", name: "Rent", description: null, category_type: "spending", children: [] },
      { id: "utilities", name: "Utilities", description: null, category_type: "spending", children: [] },
    ]},
    { id: "transport", name: "Transport", description: null, category_type: "spending", children: [
      { id: "transport-general", name: "Transport", description: null, category_type: "spending", children: [] },
    ]},
    { id: "lifestyle", name: "Lifestyle", description: null, category_type: "spending", children: [
      { id: "entertainment", name: "Entertainment", description: null, category_type: "spending", children: [] },
      { id: "shopping", name: "Shopping", description: null, category_type: "spending", children: [] },
    ]},
    { id: "health", name: "Health", description: null, category_type: "spending", children: [
      { id: "health-general", name: "Health", description: null, category_type: "spending", children: [] },
    ]},
    { id: "income", name: "Income", description: null, category_type: "income_taxable", children: [
      { id: "salary", name: "Salary", description: null, category_type: "income_taxable", children: [] },
    ]},
    { id: "transfers", name: "Transfers", description: null, category_type: "internal_transfer", children: [
      { id: "transfers-general", name: "Transfers", description: null, category_type: "internal_transfer", children: [] },
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
      category_type: body.category_type,
      created_at: now,
      updated_at: now,
    }
    if (!cat.parent_id) {
      this.mockCategoryTree.push({ id: cat.id, name: cat.name, description: cat.description, category_type: cat.category_type, children: [] })
    } else {
      const parent = this.mockCategoryTree.find(p => p.id === cat.parent_id)
      if (parent) parent.children.push({ id: cat.id, name: cat.name, description: null, category_type: cat.category_type, children: [] })
    }
    return cat
  }

  async updateCategory(id: string, body: PatchCategoryBody): Promise<Category> {
    await delay(DELAY_MS)
    const now = new Date().toISOString()
    for (const node of this.mockCategoryTree) {
      if (node.id === id) {
        if (body.name) node.name = body.name
        if (body.category_type) node.category_type = body.category_type
        return { id, name: node.name, parent_id: null, display_order: 0, is_active: true, description: null, category_type: node.category_type, created_at: now, updated_at: now }
      }
      for (const child of node.children) {
        if (child.id === id) {
          if (body.name) child.name = body.name
          if (body.category_type) child.category_type = body.category_type
          return { id, name: child.name, parent_id: node.id, display_order: 0, is_active: true, description: null, category_type: child.category_type, created_at: now, updated_at: now }
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

  async deleteTransaction(id: string): Promise<void> {
    await delay(DELAY_MS)
    const idx = MOCK_TRANSACTIONS.findIndex(t => t.id === id)
    if (idx !== -1) MOCK_TRANSACTIONS.splice(idx, 1)
  }

  async bulkDeleteTransactions(ids: string[]): Promise<void> {
    await delay(DELAY_MS)
    const set = new Set(ids)
    for (let i = MOCK_TRANSACTIONS.length - 1; i >= 0; i--) {
      if (set.has(MOCK_TRANSACTIONS[i].id)) MOCK_TRANSACTIONS.splice(i, 1)
    }
  }

  async bulkSetCategory(ids: string[], categoryId: string): Promise<void> {
    await delay(DELAY_MS)
    const set = new Set(ids)
    for (const t of MOCK_TRANSACTIONS) {
      if (set.has(t.id)) t.category_id = categoryId
    }
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

  // ── Exchange rates (date-keyed, user-owned) ───────────────────────
  //
  // Starts empty on purpose. The mock CGT response is pre-computed rather than
  // engine-derived, so it never raises `missing_exchange_rates`; these methods
  // exist so the pre-flight screen's save/list path is exercisable in mock mode.

  async getExchangeRates(filters?: {
    base?: string
    quote?: string
    start_date?: string
    end_date?: string
  }): Promise<ExchangeRate[]> {
    await delay(DELAY_MS)
    return mockExchangeRates.filter((r) => {
      if (filters?.base && r.base !== filters.base) return false
      if (filters?.quote && r.quote !== filters.quote) return false
      if (filters?.start_date && r.date < filters.start_date) return false
      if (filters?.end_date && r.date > filters.end_date) return false
      return true
    })
  }

  async createExchangeRates(rates: ExchangeRateInput[]): Promise<ExchangeRate[]> {
    await delay(DELAY_MS)
    const preferred = mockCurrencies.find((c) => c.is_preferred)?.code ?? "GBP"
    const saved: ExchangeRate[] = []
    for (const input of rates) {
      const stored: ExchangeRate = {
        base: input.base.toUpperCase(),
        quote: (input.quote ?? preferred).toUpperCase(),
        date: input.date,
        rate: input.rate,
        source: input.source ?? "user",
        updated_at: new Date().toISOString(),
      }
      const idx = mockExchangeRates.findIndex(
        (r) => r.base === stored.base && r.quote === stored.quote && r.date === stored.date,
      )
      if (idx === -1) mockExchangeRates.push(stored)
      else mockExchangeRates[idx] = stored
      saved.push(stored)
    }
    return saved
  }

  async deleteExchangeRate(base: string, quote: string, date: string): Promise<void> {
    await delay(DELAY_MS)
    const idx = mockExchangeRates.findIndex(
      (r) => r.base === base && r.quote === quote && r.date === date,
    )
    if (idx === -1) throw new Error(`No exchange rate for ${base}->${quote} on ${date}`)
    mockExchangeRates.splice(idx, 1)
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
    hints: ParseHints,
    opts?: ParseOptions
  ): Promise<IngestionPreview> {
    // Scripted progress timeline (mirrors the real llm_start -> llm_progress ->
    // post_processing -> done sequence) so the progress bar animates in mock mode.
    const emit = opts?.onProgress
    emit?.({ event: "llm_start", model: "claude-sonnet-4-6", input_tokens: 4200, task_id: "unified" })
    for (const items of [4, 11, 19, 26]) {
      await delay(DELAY_MS / 2)
      emit?.({ event: "llm_progress", output_tokens: 0, elapsed_ms: 0, items, section: "transactions", task_id: "unified" })
    }
    emit?.({ event: "phase", phase: "post_processing", message: "Checking for duplicates", task_id: null })
    await delay(DELAY_MS / 3)
    emit?.({ event: "done", total_ms: 2000 })
    const wantTx = hints.return_type.transactions
    const wantHoldings = hints.return_type.holdings.enabled
    const wantInv = hints.return_type.investments

    const txRows = wantTx
      ? [
          { index: 0, date: "2026-05-15T00:00:00", description: "TfL", amount: "-2.80", currency: "GBP", status: "new" as const, existing_id: null, existing_description: null, error_reason: null, category_id: "transport-general", category_confidence: 0.95, source_document_ids: [] },
          { index: 1, date: "2026-05-15T00:00:00", description: "Lidl", amount: "-23.45", currency: "GBP", status: "duplicate" as const, existing_id: "tx_abc123", existing_description: "Lidl", error_reason: null, category_id: "groceries", category_confidence: 0.97, source_document_ids: [] },
          { index: 2, date: "2026-05-16T00:00:00", description: "Pret a Manger", amount: "-4.50", currency: "GBP", status: "new" as const, existing_id: null, existing_description: null, error_reason: null, category_id: "dining", category_confidence: 0.78, source_document_ids: [] },
          { index: 3, date: "2026-05-17T00:00:00", description: "Spotify", amount: "-9.99", currency: "GBP", status: "new" as const, existing_id: null, existing_description: null, error_reason: null, category_id: "entertainment", category_confidence: 0.45, source_document_ids: [] },
        ]
      : []
    const txPayload: ImportPayload | null = wantTx
      ? {
          account_id: accountId,
          transactions: [
            { date: "2026-05-15T00:00:00", description: "TfL", amount: "-2.80", currency: "GBP", category_id: "transport-general", category_source: "agent" satisfies CategorySource, notes: null, is_recurring: null, exclude_from_summary: null, source_document_ids: [] },
            { date: "2026-05-16T00:00:00", description: "Pret a Manger", amount: "-4.50", currency: "GBP", category_id: "dining", category_source: "agent" satisfies CategorySource, notes: null, is_recurring: null, exclude_from_summary: null, source_document_ids: [] },
            { date: "2026-05-17T00:00:00", description: "Spotify", amount: "-9.99", currency: "GBP", category_id: "entertainment", category_source: "agent" satisfies CategorySource, notes: null, is_recurring: true, exclude_from_summary: null, source_document_ids: [] },
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
            { account_id: accountId, event_type: "buy", symbol: "AAPL", date: "2026-04-10T14:30:00", quantity: "10.0000", price_per_share: "185.20", fee: "0.00", currency: "USD", fee_currency: null, notes: null, source_document_ids: [] },
          ],
        }
      : null

    const newCount = (n: number, d = 0) => n - d
    const _ = files // referenced to keep ts-rs happy
    void _
    const calls = [
      {
        parser: "unified",
        agent: "sonnet" as const,
        model: "claude-sonnet-4-6",
        input_tokens: BigInt(8420),
        output_tokens: BigInt(1180),
        duration_ms: BigInt(2340),
        amount: "0.0430",
        currency: "USD",
      },
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

  // ── Investments ───────────────────────────────────────────────────

  async listInvestments(
    accountId?: string,
    symbol?: string,
    eventType?: string
  ): Promise<InvestmentEvent[]> {
    await delay(DELAY_MS)
    return this.investments
      .filter((e) => !accountId || e.account_id === accountId)
      .filter((e) => !symbol || e.symbol === symbol)
      .filter((e) => !eventType || e.event_type === eventType)
      .slice()
      .sort((a, b) => b.date.localeCompare(a.date))
  }

  async createInvestment(body: CreateInvestmentEventBody): Promise<InvestmentEvent> {
    await delay(DELAY_MS)
    const event = mockEvent(
      `mock_inv_${Math.random().toString(36).slice(2, 10)}`,
      body.account_id,
      body.event_type as InvestmentEventType,
      body.symbol,
      body.date,
      body.quantity,
      body.price_per_share,
      body.fee,
      body.currency,
      body.fee_currency,
      body.notes,
    )
    this.investments.unshift(event)
    return event
  }

  async updateInvestment(id: string, body: PatchInvestmentEventBody): Promise<InvestmentEvent> {
    await delay(DELAY_MS)
    const idx = this.investments.findIndex((e) => e.id === id)
    if (idx === -1) throw new Error(`investment event ${id} not found`)
    const cur = this.investments[idx]
    const updated: InvestmentEvent = {
      ...cur,
      event_type: (body.event_type as InvestmentEventType | null) ?? cur.event_type,
      symbol: body.symbol ?? cur.symbol,
      date: body.date ?? cur.date,
      quantity: body.quantity ?? cur.quantity,
      price_per_share: body.price_per_share ?? cur.price_per_share,
      fee: body.fee !== undefined ? body.fee : cur.fee,
      currency: body.currency ?? cur.currency,
      fee_currency: body.fee_currency !== undefined ? body.fee_currency : cur.fee_currency,
      notes: body.notes !== undefined ? body.notes : cur.notes,
    }
    this.investments[idx] = updated
    return updated
  }

  async deleteInvestment(id: string): Promise<void> {
    await delay(DELAY_MS)
    this.investments = this.investments.filter((e) => e.id !== id)
  }

  async getInvestmentPools(_profileId?: string): Promise<S104PoolState[]> {
    await delay(DELAY_MS)
    return derivePools(this.investments)
  }

  async getInvestmentHistory(
    start: string,
    end: string,
    _granularity: Granularity = "monthly",
    _profileId?: string,
    _accountIds: string[] = [],
  ): Promise<InvestmentHistoryRow[]> {
    await delay(DELAY_MS)
    const months = getMonthsInRange(start, end)
    return months.map((month, i) => {
      // Demo the no-data gap: nothing before the third month in range.
      if (i < 2) return { period: month, net_invested: null, market_value: null }
      const invested = 100000 + i * 2000
      const value = invested * (1 + 0.03 * (i - 1))
      return { period: month, net_invested: invested.toFixed(2), market_value: value.toFixed(2) }
    })
  }

  // ── Documents ─────────────────────────────────────────────────────

  async listDocuments(includeRefs = false): Promise<DocumentSummary[]> {
    await delay(DELAY_MS)
    // Match the real backend: the plain list never carries the exact count;
    // `include=refs` populates it for every row (orphans resolve to 0).
    return this.documents.map((d) => ({
      ...d,
      reference_count: includeRefs ? (d.orphaned ? 0 : (this.documentRefCounts[d.id] ?? 0)) : null,
    }))
  }

  async getDocument(id: string): Promise<DocumentSummary> {
    await delay(DELAY_MS)
    const doc = this.documents.find((d) => d.id === id)
    if (!doc) throw new Error(`document ${id} not found`)
    const count = doc.orphaned ? 0 : (this.documentRefCounts[id] ?? 0)
    return { ...doc, reference_count: count }
  }

  async uploadDocuments(files: File[], accountId?: string): Promise<DocumentSummary[]> {
    await delay(DELAY_MS)
    const created = files.map((f) => {
      const id = `mock_doc_${Math.random().toString(36).slice(2, 10)}`
      this.documentRefCounts[id] = 0
      const doc: DocumentSummary = {
        id,
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
    const refs = doc ? (doc.orphaned ? 0 : (this.documentRefCounts[id] ?? 0)) : 0
    if (doc && refs > 0 && !force) {
      // Synthesize a plausible breakdown for the confirm dialog.
      throw new DocumentReferencedError({
        transactions: refs,
        holdings: 0,
        investments: 0,
      })
    }
    this.documents = this.documents.filter((d) => d.id !== id)
    return {
      deleted: true,
      unlinked: { transactions: refs, holdings: 0, investments: 0 },
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
    original_currency: "GBP",
    current_shares: "120",
    total_allowable_expenditure: "8640.00",
    average_cost_per_share: "72.00",
  },
  {
    symbol: "AAPL",
    original_currency: "USD",
    current_shares: "45",
    total_allowable_expenditure: "5688.00",
    average_cost_per_share: "126.40",
  },
]

/**
 * Mirrors the backend's `group_disposals` (backend/src/server/routes/capital_gains.rs):
 * rolls `realized_events` up by `(symbol, disposal_date)` into one row per actual sale,
 * summing the matched-bucket figures rather than just counting rows.
 */
function groupDisposals(events: CgtRealizedEvent[]): CgtDisposalGroup[] {
  const groups = new Map<string, CgtDisposalGroup>()
  for (const ev of events) {
    // Key on the calendar DAY, not the timestamp — matching the backend. UK capital
    // gains are reckoned by day, so two sells of one holding on one date are a single
    // disposal; keying on the full datetime would overstate the SA108 disposal count.
    const day = ev.disposal_date.slice(0, 10)
    const key = `${ev.symbol} ${day}`
    let group = groups.get(key)
    if (!group) {
      group = {
        symbol: ev.symbol,
        disposal_date: day,
        quantity: "0",
        proceeds: "0",
        cost_basis: "0",
        gain_loss: "0",
        original_currency: ev.original_currency,
        events: [],
      }
      groups.set(key, group)
    }
    group.quantity = (Number.parseFloat(group.quantity) + Number.parseFloat(ev.quantity)).toString()
    group.proceeds = (Number.parseFloat(group.proceeds) + Number.parseFloat(ev.proceeds)).toFixed(2)
    group.cost_basis = (Number.parseFloat(group.cost_basis) + Number.parseFloat(ev.cost_basis)).toFixed(2)
    group.gain_loss = (Number.parseFloat(group.gain_loss) + Number.parseFloat(ev.gain_loss)).toFixed(2)
    group.events.push(ev)
  }
  return Array.from(groups.values())
}

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
    disposal_groups: groupDisposals(events),
    pools: MOCK_POOLS,
  }
}

// ── Mock investment events ────────────────────────────────────────────────────

function mockEvent(
  id: string,
  accountId: string,
  eventType: InvestmentEventType,
  symbol: string,
  date: string,
  quantity: string,
  pricePerShare: string,
  fee: string | null,
  currency: string,
  feeCurrency: string | null = null,
  notes: string | null = null,
): InvestmentEvent {
  return {
    id,
    account_id: accountId,
    event_type: eventType,
    symbol,
    date,
    quantity,
    price_per_share: pricePerShare,
    fee,
    currency,
    fee_currency: feeCurrency,
    notes,
    fingerprint: `mock_fp_${id}`,
    created_at: date,
    source_document_ids: [],
  }
}

/**
 * Derives a plausible S104 pool snapshot from the mock events: average-cost
 * accumulation on buy/vest/transfer-in, proportional cost removal on sell.
 * Not the real HMRC engine, just enough for the Overview table to render.
 */
function derivePools(events: InvestmentEvent[]): S104PoolState[] {
  const pools = new Map<string, { shares: number; cost: number; currency: string }>()
  const ordered = [...events].sort((a, b) => a.date.localeCompare(b.date))

  for (const e of ordered) {
    const pool = pools.get(e.symbol) ?? { shares: 0, cost: 0, currency: e.currency }
    const qty = parseFloat(e.quantity)
    const price = parseFloat(e.price_per_share)
    const fee = e.fee ? parseFloat(e.fee) : 0

    if (e.event_type === "sell" || e.event_type === "withhold") {
      const avg = pool.shares > 0 ? pool.cost / pool.shares : 0
      const removed = Math.min(qty, pool.shares)
      pool.shares -= removed
      pool.cost -= avg * removed
    } else {
      pool.shares += qty
      pool.cost += qty * price + fee
    }
    pools.set(e.symbol, pool)
  }

  return Array.from(pools.entries())
    .filter(([, p]) => p.shares > 0.0001)
    .map(([symbol, p]) => ({
      symbol,
      original_currency: p.currency,
      current_shares: p.shares.toFixed(4),
      total_allowable_expenditure: p.cost.toFixed(2),
      average_cost_per_share: (p.shares > 0 ? p.cost / p.shares : 0).toFixed(2),
    }))
    .sort((a, b) => a.symbol.localeCompare(b.symbol))
}
