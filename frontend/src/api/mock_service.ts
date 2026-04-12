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
  Profile,
  SpendingGridRow,
  Transaction,
  TransactionFilters,
} from "@/types"
import type { ApiService } from "./service"
import {
  MOCK_PROFILES,
  MOCK_ACCOUNTS,
  MOCK_TRANSACTIONS,
  MOCK_HOLDINGS,
  MOCK_BUDGETS,
  MOCK_PORTFOLIO_SNAPSHOTS,
} from "@/data"
import { delay, getMonthFromDate, getMonthsInRange } from "@/lib/utils"

const DELAY_MS = 1000

// Available/unavailable account type classification
const AVAILABLE_TYPES = new Set(["checking", "savings", "investment", "cash", "credit"])
// Liability types that subtract from unavailable wealth (e.g. mortgage offsets property value)
const UNAVAILABLE_LIABILITY_TYPES = new Set(["mortgage"])

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
        budgeted: b.amount,
        actual: actual.toFixed(2),
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
        section: getSection(cat),
        months: monthValues,
        average: avg.toFixed(2),
        budget: budget?.amount ?? null,
        total: total.toFixed(2),
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

  async updateBudget(req: BudgetUpdateRequest): Promise<void> {
    await delay(DELAY_MS)
    const existing = MOCK_BUDGETS.find(
      (b) => b.month === req.month && b.category === req.category
    )
    if (existing) {
      existing.amount = req.amount
    } else {
      MOCK_BUDGETS.push({
        month: req.month,
        category: req.category,
        amount: req.amount,
      })
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
      if ((a.type === "credit" || a.type === "mortgage") && bal > 0) {
        totalLiabilities += bal
      } else {
        totalAssets += Math.abs(bal)
      }
      if (AVAILABLE_TYPES.has(a.type)) {
        availableWealth += a.type === "credit" ? -bal : bal
      } else if (UNAVAILABLE_LIABILITY_TYPES.has(a.type)) {
        unavailableWealth -= bal // Mortgage subtracts from unavailable (offsets property)
      } else {
        unavailableWealth += bal
      }
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

    // By sector (simplified)
    const bySector = new Map<string, number>()
    for (const a of accounts) {
      const bal = parseFloat(a.balance ?? "0")
      let sector: string
      if (a.type === "investment") sector = "Stocks"
      else if (a.type === "pension") sector = "Pension"
      else if (a.type === "property" || a.type === "mortgage") sector = "Property"
      else if (a.type === "savings" || a.type === "checking" || a.type === "cash")
        sector = "Cash"
      else sector = "Other"
      bySector.set(sector, (bySector.get(sector) ?? 0) + bal)
    }

    function toBreakdown(map: Map<string, number>) {
      const total = Array.from(map.values()).reduce((s, v) => s + v, 0)
      return Array.from(map.entries())
        .map(([label, val]) => ({
          label,
          total: val.toFixed(2),
          percent: total > 0 ? Math.round((val / total) * 100) : 0,
        }))
        .sort((a, b) => parseFloat(b.total) - parseFloat(a.total))
    }

    // Monthly snapshots aggregated
    const monthMap = new Map<string, number>()
    const accountIds = new Set(accounts.map((a) => a.id))
    for (const snap of MOCK_PORTFOLIO_SNAPSHOTS) {
      if (!accountIds.has(snap.account_id)) continue
      const month = getMonthFromDate(snap.snapshot_date)
      monthMap.set(month, (monthMap.get(month) ?? 0) + parseFloat(snap.balance))
    }

    return {
      net_worth: netWorth.toFixed(2),
      currency: "GBP",
      as_of: "2026-03-20",
      total_assets: totalAssets.toFixed(2),
      total_liabilities: totalLiabilities.toFixed(2),
      available_wealth: availableWealth.toFixed(2),
      unavailable_wealth: unavailableWealth.toFixed(2),
      accounts,
      by_type: toBreakdown(byType),
      by_institution: toBreakdown(byInst),
      by_sector: toBreakdown(bySector),
    }
  }

  async getPortfolioHistory(
    start?: string,
    end?: string
  ): Promise<PortfolioHistoryRow[]> {
    await delay(DELAY_MS)

    // Aggregate snapshots by month, split by available/unavailable
    const months = new Map<
      string,
      { available: number; unavailable: number }
    >()

    for (const snap of MOCK_PORTFOLIO_SNAPSHOTS) {
      const month = getMonthFromDate(snap.snapshot_date)
      if (start && month < start.substring(0, 7)) continue
      if (end && month > end.substring(0, 7)) continue

      const account = MOCK_ACCOUNTS.find((a) => a.id === snap.account_id)
      if (!account) continue

      if (!months.has(month)) months.set(month, { available: 0, unavailable: 0 })
      const entry = months.get(month)!
      const bal = parseFloat(snap.balance)

      if (AVAILABLE_TYPES.has(account.type)) {
        entry.available += bal
      } else if (UNAVAILABLE_LIABILITY_TYPES.has(account.type)) {
        entry.unavailable -= bal // Mortgage subtracts from unavailable
      } else {
        entry.unavailable += bal
      }
    }

    return Array.from(months.entries())
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([month, { available, unavailable }]) => ({
        month,
        available_wealth: available.toFixed(2),
        unavailable_wealth: unavailable.toFixed(2),
        total_wealth: (available + unavailable).toFixed(2),
      }))
  }

  async getHoldings(accountId: string): Promise<Holding[]> {
    await delay(DELAY_MS)
    return MOCK_HOLDINGS.filter((h) => h.account_id === accountId)
  }

  async getCashFlow(start?: string, end?: string): Promise<CashFlowMonth[]> {
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
        spending: spending.toFixed(2),
      }))
  }

  async getAccountSnapshots(
    start?: string,
    end?: string
  ): Promise<PortfolioSnapshot[]> {
    await delay(DELAY_MS)
    return MOCK_PORTFOLIO_SNAPSHOTS.filter((s) => {
      const month = getMonthFromDate(s.snapshot_date)
      if (start && month < start.substring(0, 7)) return false
      if (end && month > end.substring(0, 7)) return false
      return true
    })
  }

  async exportData(format: string): Promise<void> {
    await delay(DELAY_MS)
    console.log(`[Mock] Export requested: format=${format}`)
  }
}
