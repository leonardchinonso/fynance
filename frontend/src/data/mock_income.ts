import type { CgtLink } from "@/bindings/CgtLink"
import type { DonationSummary } from "@/bindings/DonationSummary"
import type { EmploymentSourceSummary } from "@/bindings/EmploymentSourceSummary"
import type { IncomeCheck } from "@/bindings/IncomeCheck"
import type { IncomeCheckPayload } from "@/bindings/IncomeCheckPayload"
import type { IncomeLineItem } from "@/bindings/IncomeLineItem"
import type { IncomeLineItemInput } from "@/bindings/IncomeLineItemInput"
import type { IncomeLineType } from "@/bindings/IncomeLineType"
import type { IncomeRecord } from "@/bindings/IncomeRecord"
import type { IncomeRecordPayload } from "@/bindings/IncomeRecordPayload"
import type { IncomeSource } from "@/bindings/IncomeSource"
import type { IncomeSourcePayload } from "@/bindings/IncomeSourcePayload"
import type { IncomeSyncPeriod } from "@/bindings/IncomeSyncPeriod"
import type { IncomeSyncRequest } from "@/bindings/IncomeSyncRequest"
import type { IncomeSyncResult } from "@/bindings/IncomeSyncResult"
import type { InterestSummary } from "@/bindings/InterestSummary"
import type { InvestmentEvent } from "@/bindings/InvestmentEvent"
import type { LineTypeInfo } from "@/bindings/LineTypeInfo"
import type { PatchIncomeCheckBody } from "@/bindings/PatchIncomeCheckBody"
import type { PatchIncomeRecordBody } from "@/bindings/PatchIncomeRecordBody"
import type { PatchIncomeSourceBody } from "@/bindings/PatchIncomeSourceBody"
import type { RsuVestSummary } from "@/bindings/RsuVestSummary"
import type { SelfEmploymentSummary } from "@/bindings/SelfEmploymentSummary"
import type { TaxYearSummary } from "@/bindings/TaxYearSummary"
import type { VestCandidate } from "@/bindings/VestCandidate"
import { fromCents, sumMoney, toCents } from "@/lib/money"

// Synthetic income data for the Tax page in mock mode. Generic names and
// figures only; the shapes mirror the backend's income model (plan 30).

const USD_TO_GBP = 0.8
const TRADING_ALLOWANCE_CENTS = 100_000

function row(
  line_type: IncomeLineType, source_type: LineTypeInfo["source_type"], label: string,
  summed: boolean, form = "", box_ref = "", sign = 1,
): LineTypeInfo {
  return { line_type, source_type, label, summed, sign, form, box_ref, note: "" }
}

export const MOCK_LINE_TYPES: LineTypeInfo[] = [
  row("taxable_pay", "employment", "Taxable pay", true, "SA102", "1"),
  row("gross_pay", "employment", "Gross pay", true, "SA102", "1"),
  row("paye_tax", "employment", "PAYE tax", true, "SA102", "2"),
  row("employee_ni", "employment", "Employee NI", false),
  row("student_loan", "employment", "Student loan", true, "SA100", "Student loan repayments deducted by employer"),
  row("postgraduate_loan", "employment", "Postgraduate loan", true, "SA100", "Student loan repayments deducted by employer"),
  row("employee_pension", "employment", "Employee pension", false),
  row("rsu_vest_value", "employment", "RSU vest value", false),
  row("rsu_tax", "employment", "RSU tax", false),
  row("net_pay", "employment", "Net pay", false),
  row("other_payment", "employment", "Other payment", false),
  row("other_deduction", "employment", "Other deduction", false),
  row("sales", "self_employment", "Sales", true, "SA103S", "9"),
  row("refunds", "self_employment", "Refunds", true, "SA103S", "9", -1),
  row("platform_fees", "self_employment", "Platform fees", true, "SA103S", "19"),
  row("platform_postage", "self_employment", "Platform postage", true, "SA103S", "18"),
  row("cost_of_goods", "self_employment", "Cost of goods", true, "SA103S", "11"),
  row("postage", "self_employment", "Postage", true, "SA103S", "18"),
  row("packaging", "self_employment", "Packaging", true, "SA103S", "11"),
  row("other_expenses", "self_employment", "Other expenses", true, "SA103S", "19"),
  row("costing_materials", "self_employment", "Costing: materials", false),
  row("costing_packaging", "self_employment", "Costing: packaging", false),
  row("costing_postage", "self_employment", "Costing: postage", false),
  row("costing_other", "self_employment", "Costing: other", false),
  row("rent_received", "property", "Rent received", false),
  row("property_expenses", "property", "Property expenses", false),
]

const INFO = new Map(MOCK_LINE_TYPES.map((i) => [i.line_type, i]))

/** Business-account outflows by category in the seeded tax year (stand-in for the bank query). */
const BANK_OUTFLOWS = [
  { category_id: "shopping", name: "Shopping", total: "212.40" },
  { category_id: "transport-general", name: "Transport", total: "58.30" },
]

/** Seeded interest and donations per profile. */
const INTEREST: Record<string, InterestSummary> = {
  alex: {
    taxable_total: "84.20", non_taxable_total: "120.00",
    by_account: [
      { account_id: "lloyds-savings", account_name: "Lloyds Savings", taxable: "84.20", non_taxable: "0.00", joint_share_of: 1 },
      { account_id: "premium-bonds", account_name: "Premium Bonds", taxable: "0.00", non_taxable: "120.00", joint_share_of: 1 },
    ],
  },
  sam: {
    taxable_total: "18.35", non_taxable_total: "0.00",
    by_account: [
      { account_id: "joint-savings", account_name: "Joint Savings", taxable: "18.35", non_taxable: "0.00", joint_share_of: 2 },
    ],
  },
}
const DONATIONS: Record<string, DonationSummary> = {
  alex: { gift_aid_total: "240.00", non_gift_aid_total: "15.00" },
}

function taxYearOf(date: string): string {
  const [y, m, d] = date.split("-").map(Number)
  const start = m > 4 || (m === 4 && d >= 6) ? y : y - 1
  return `${start}-${String((start + 1) % 100).padStart(2, "0")}`
}

function ymd(y: number, m: number, d: number): string {
  return `${y}-${String(m).padStart(2, "0")}-${String(d).padStart(2, "0")}`
}

function lastDay(y: number, m: number): number {
  return new Date(y, m, 0).getDate()
}

function toItems(prefix: string, inputs: IncomeLineItemInput[], currency: string): IncomeLineItem[] {
  return inputs.map((i, n) => ({
    id: `${prefix}_li${n}`,
    line_type: i.line_type,
    amount: i.amount,
    currency: i.currency ?? currency,
    basis: i.basis,
    label: i.label,
    investment_event_id: i.investment_event_id,
  }))
}

function item(line_type: IncomeLineType, amount: string): IncomeLineItemInput {
  return { line_type, amount, basis: "period" }
}

let nextId = 1
function newId(kind: string): string {
  return `mock_${kind}_${nextId++}`
}

function monthsInTaxYear(startYear: number): { y: number; m: number }[] {
  return Array.from({ length: 12 }, (_, i) => ({
    y: startYear + (i >= 9 ? 1 : 0),
    m: ((3 + i) % 12) + 1,
  }))
}

/** One synthetic shop month: figures vary with the month number so periods differ. */
function shopFigures(m: number): IncomeLineItemInput[] {
  const sales = 600 + m * 41
  return [
    item("sales", `${sales}.00`),
    item("refunds", `${10 + m}.00`),
    item("platform_fees", `${(sales * 0.03).toFixed(2)}`),
    item("platform_postage", `${20 + m}.50`),
    item("costing_materials", `${Math.round(sales * 0.22)}.00`),
  ]
}

function sameItems(a: IncomeLineItem[], b: IncomeLineItemInput[]): boolean {
  if (a.length !== b.length) return false
  return a.every((x, i) => x.line_type === b[i].line_type && toCents(x.amount) === toCents(b[i].amount))
}

function httpError(code: string): Error {
  return new Error(code)
}

export class MockIncomeStore {
  sources: IncomeSource[] = []
  records: IncomeRecord[] = []
  checks: IncomeCheck[] = []
  readonly vestEventId = "mock_inv_alex_vest_current"
  readonly seedTaxYear: string
  private readonly startYear: number

  constructor(now: Date = new Date()) {
    const today = ymd(now.getFullYear(), now.getMonth() + 1, now.getDate())
    this.seedTaxYear = taxYearOf(today)
    this.startYear = Number(this.seedTaxYear.slice(0, 4))
    this.seed(today)
  }

  /** The vest event the seeded RSU payslip is meant to link to. */
  vestEventDate(): string {
    return ymd(this.startYear, 7, 31)
  }

  private now(): string {
    return new Date().toISOString()
  }

  private seed(today: string): void {
    const ts = this.now()
    const base = { identifiers: {}, account_ids: [], expense_category_map: {}, active: true, created_at: ts, updated_at: ts }
    this.sources = [
      { ...base, id: "mock_src_employer_a", profile_id: "alex", source_type: "employment", name: "Employer A", identifiers: { paye_reference: "123/AB456" } },
      {
        ...base, id: "mock_src_craft_shop", profile_id: "alex", source_type: "self_employment",
        name: "Example Craft Shop", identifiers: { accounting_basis: "cash" },
        account_ids: ["revolut-current"], expense_category_map: { shopping: "cost_of_goods" }, sync_kind: "shop_app",
      },
      { ...base, id: "mock_src_employer_b", profile_id: "sam", source_type: "employment", name: "Employer B" },
    ]

    const months = monthsInTaxYear(this.startYear).filter(({ y, m }) => ymd(y, m, 1) <= today).slice(0, 7)
    months.forEach(({ y, m }, i) => {
      const end = ymd(y, m, lastDay(y, m))
      const lines: IncomeLineItemInput[] = [
        item("taxable_pay", "3200.00"), item("paye_tax", "520.00"), item("employee_ni", "210.00"),
        item("student_loan", "95.00"), item("net_pay", "2375.00"),
      ]
      if (m === 7) lines.push(item("rsu_vest_value", "2100.00"), item("rsu_tax", "840.00"))
      this.records.push(this.buildRecord(`mock_rec_a_${i}`, "mock_src_employer_a", ymd(y, m, 1), end, {
        pay_date: end, origin: "parse", lines,
      }))
      this.records.push(this.buildRecord(`mock_rec_b_${i}`, "mock_src_employer_b", ymd(y, m, 1), end, {
        pay_date: end, origin: "manual", lines: [item("taxable_pay", "1500.00"), item("paye_tax", "180.00")],
      }))
    })

    const shopMonths = months.slice(0, 5)
    shopMonths.forEach(({ y, m }, i) => {
      const start = i === 0 && m === 4 ? ymd(y, 4, 6) : ymd(y, m, 1)
      const end = ymd(y, m, lastDay(y, m))
      const lines = shopFigures(m)
      if (i === 2) lines[0] = item("sales", "9999.00")
      this.records.push(this.buildRecord(`mock_rec_shop_${i}`, "mock_src_craft_shop", start, end, {
        origin: "shop_sync", external_ref: `${start}..${end}`, lines,
      }))
    })

    this.checks = [
      {
        id: "mock_chk_p60_a", source_id: "mock_src_employer_a", tax_year: this.seedTaxYear, check_type: "p60",
        figures: [
          { line_type: "taxable_pay", amount: "23400.00", currency: "GBP" },
          { line_type: "paye_tax", amount: "3700.00", currency: "GBP" },
        ],
        document_ids: [], created_at: ts, updated_at: ts,
      },
    ]
  }

  private buildRecord(
    id: string, source_id: string, period_start: string, period_end: string,
    extra: {
      pay_date?: string; origin: IncomeRecord["origin"]; external_ref?: string
      kind?: IncomeRecord["kind"]; lines: IncomeLineItemInput[]; notes?: string
    },
  ): IncomeRecord {
    const ts = this.now()
    return {
      id, source_id, kind: extra.kind ?? "period", period_start, period_end, pay_date: extra.pay_date,
      tax_year: taxYearOf(extra.pay_date ?? period_end), currency: "GBP", transaction_ids: [], document_ids: [],
      origin: extra.origin, external_ref: extra.external_ref, notes: extra.notes,
      line_items: toItems(id, extra.lines, "GBP"), created_at: ts, updated_at: ts,
    }
  }

  // ── Sources ─────────────────────────────────────────────────────

  listSources(profileId?: string): IncomeSource[] {
    return this.sources.filter((s) => !profileId || s.profile_id === profileId)
  }

  createSource(body: IncomeSourcePayload): IncomeSource {
    const ts = this.now()
    const src: IncomeSource = { ...body, id: newId("src"), created_at: ts, updated_at: ts }
    this.sources.push(src)
    return src
  }

  updateSource(id: string, body: PatchIncomeSourceBody): IncomeSource {
    const idx = this.sources.findIndex((s) => s.id === id)
    if (idx === -1) throw httpError("source_not_found")
    const next = { ...this.sources[idx], updated_at: this.now() } as unknown as Record<string, unknown>
    for (const [k, v] of Object.entries(body)) {
      if (v === undefined) continue
      if (v === null) delete next[k]
      else next[k] = v
    }
    this.sources[idx] = next as unknown as IncomeSource
    return this.sources[idx]
  }

  deleteSource(id: string): void {
    if (this.records.some((r) => r.source_id === id)) throw httpError("source_has_records")
    this.sources = this.sources.filter((s) => s.id !== id)
    this.checks = this.checks.filter((c) => c.source_id !== id)
  }

  // ── Records ─────────────────────────────────────────────────────

  listRecords(f: { profileId?: string; sourceId?: string; taxYear?: string }): IncomeRecord[] {
    const inProfile = new Set(this.listSources(f.profileId).map((s) => s.id))
    return this.records
      .filter((r) => inProfile.has(r.source_id) && (!f.sourceId || r.source_id === f.sourceId))
      .filter((r) => !f.taxYear || r.tax_year === f.taxYear)
      .slice()
      .sort((a, b) => (a.pay_date ?? a.period_end).localeCompare(b.pay_date ?? b.period_end))
  }

  createRecord(body: IncomeRecordPayload): IncomeRecord {
    const src = this.sources.find((s) => s.id === body.source_id)
    if (!src) throw httpError("source_not_found")
    if (body.period_end < body.period_start) throw httpError("invalid_period")
    const id = newId("rec")
    const rec = this.buildRecord(id, body.source_id, body.period_start, body.period_end, {
      pay_date: body.pay_date, origin: "manual", lines: body.line_items, notes: body.notes,
    })
    rec.currency = body.currency
    this.records.push(rec)
    return rec
  }

  updateRecord(id: string, body: PatchIncomeRecordBody): IncomeRecord {
    const idx = this.records.findIndex((r) => r.id === id)
    if (idx === -1) throw httpError("record_not_found")
    const cur = this.records[idx]
    const next: IncomeRecord = { ...cur, updated_at: this.now() }
    if (body.period_start !== undefined) next.period_start = body.period_start
    if (body.period_end !== undefined) next.period_end = body.period_end
    if (body.pay_date !== undefined) next.pay_date = body.pay_date ?? undefined
    if (body.currency !== undefined) next.currency = body.currency
    if (body.notes !== undefined) next.notes = body.notes ?? undefined
    if (body.line_items !== undefined) next.line_items = toItems(id, body.line_items, next.currency)
    next.tax_year = taxYearOf(next.pay_date ?? next.period_end)
    this.records[idx] = next
    return next
  }

  deleteRecord(id: string): void {
    this.records = this.records.filter((r) => r.id !== id)
  }

  // ── Checks ──────────────────────────────────────────────────────

  listChecks(f: { profileId?: string; taxYear?: string }): IncomeCheck[] {
    const inProfile = new Set(this.listSources(f.profileId).map((s) => s.id))
    return this.checks.filter((c) => inProfile.has(c.source_id) && (!f.taxYear || c.tax_year === f.taxYear))
  }

  createCheck(body: IncomeCheckPayload): IncomeCheck {
    if (this.checks.some((c) => c.source_id === body.source_id && c.tax_year === body.tax_year && c.check_type === body.check_type)) {
      throw httpError("check_exists")
    }
    const ts = this.now()
    const check: IncomeCheck = { ...body, id: newId("chk"), created_at: ts, updated_at: ts }
    this.checks.push(check)
    return check
  }

  updateCheck(id: string, body: PatchIncomeCheckBody): IncomeCheck {
    const idx = this.checks.findIndex((c) => c.id === id)
    if (idx === -1) throw httpError("check_not_found")
    const next = { ...this.checks[idx], updated_at: this.now() }
    if (body.figures !== undefined) next.figures = body.figures
    if (body.document_ids !== undefined) next.document_ids = body.document_ids
    if (body.notes !== undefined) next.notes = body.notes ?? undefined
    this.checks[idx] = next
    return next
  }

  deleteCheck(id: string): void {
    this.checks = this.checks.filter((c) => c.id !== id)
  }

  // ── Year-end adjustment ─────────────────────────────────────────

  createAdjustment(sourceId: string, taxYear: string): IncomeRecord {
    const src = this.sources.find((s) => s.id === sourceId)
    if (!src) throw httpError("source_not_found")
    const check = this.checks.find((c) => c.source_id === sourceId && c.tax_year === taxYear)
    if (!check) throw httpError("no_check")
    const recs = this.records.filter((r) => r.source_id === sourceId && r.tax_year === taxYear)
    if (recs.some((r) => r.kind === "year_end_adjustment")) throw httpError("adjustment_exists")
    const period = recs.filter((r) => r.kind === "period")
    const lines: IncomeLineItemInput[] = []
    const diff = (lt: IncomeLineType, target: number, have: number) => {
      if (target - have !== 0) lines.push({ line_type: lt, amount: fromCents(target - have), basis: "period", currency: "GBP", label: "Year-end adjustment" })
    }
    if (src.source_type === "employment") {
      const s = this.employmentSums(period)
      diff("taxable_pay", this.figure(check, ["taxable_pay", "gross_pay"]), s.pay)
      diff("paye_tax", this.figure(check, ["paye_tax"]), s.tax)
    } else {
      const s = this.businessSums(src, period, taxYear)
      diff("sales", this.figure(check, ["sales"]) - this.figure(check, ["refunds"]), s.turnover)
      diff("other_expenses", this.checkExpenses(check), s.expenses)
    }
    if (lines.length === 0) throw httpError("no_gap")
    const [start] = ukStart(taxYear)
    const rec = this.buildRecord(newId("adj"), sourceId, start, ukEnd(taxYear), { origin: "adjustment", kind: "year_end_adjustment", lines })
    this.records.push(rec)
    return rec
  }

  // ── Shop sync ───────────────────────────────────────────────────

  sync(sourceId: string, body: IncomeSyncRequest): IncomeSyncResult {
    const src = this.sources.find((s) => s.id === sourceId)
    if (!src) throw httpError("source_not_found")
    if (src.source_type !== "self_employment" || src.sync_kind !== "shop_app") throw httpError("sync_not_supported")
    const periods: IncomeSyncPeriod[] = []
    const [fy, fm] = body.from.split("-").map(Number)
    const [ty, tm] = body.to.split("-").map(Number)
    const monthCount = (ty - fy) * 12 + (tm - fm) + 1
    for (let n = 0; n < monthCount; n++) {
      const y = fy + Math.floor((fm - 1 + n) / 12)
      const m = ((fm - 1 + n) % 12) + 1
      const start = m === 4 ? ymd(y, 4, 6) : ymd(y, m, 1)
      const end = ymd(y, m, lastDay(y, m))
      if (end < body.from || start > body.to) continue
      const lines = shopFigures(m)
      const external_ref = `${start}..${end}`
      const existing = this.records.find((r) => r.source_id === sourceId && r.origin === "shop_sync" && r.external_ref === external_ref)
      const status = !existing ? "created" : sameItems(existing.line_items, lines) ? "unchanged" : "updated"
      periods.push({ external_ref, status, line_items: lines })
    }
    if (!body.dry_run) {
      for (const p of periods) {
        if (p.status === "unchanged") continue
        const [start, end] = p.external_ref.split("..")
        const idx = this.records.findIndex((r) => r.source_id === sourceId && r.origin === "shop_sync" && r.external_ref === p.external_ref)
        const rec = this.buildRecord(idx === -1 ? newId("rec") : this.records[idx].id, sourceId, start, end, {
          origin: "shop_sync", external_ref: p.external_ref, lines: p.line_items,
        })
        if (idx === -1) this.records.push(rec)
        else this.records[idx] = rec
      }
    }
    const count = (s: IncomeSyncPeriod["status"]) => periods.filter((p) => p.status === s).length
    return {
      dry_run: body.dry_run, created: count("created"), updated: count("updated"), unchanged: count("unchanged"),
      periods,
      late_refunds: [{ order_month: ukMonthBefore(body.from), refund_date: ymd(fy, fm, 10), amount: "14.00" }],
      warnings: ["2 orders in the period have no card fee recorded"],
    }
  }

  // ── Vest candidates ─────────────────────────────────────────────

  vestCandidates(recordId: string, investments: InvestmentEvent[], accountsOfProfile: (profileId: string) => string[]): VestCandidate[] {
    const rec = this.records.find((r) => r.id === recordId)
    if (!rec) throw httpError("record_not_found")
    const src = this.sources.find((s) => s.id === rec.source_id)
    const accounts = new Set(src ? accountsOfProfile(src.profile_id) : [])
    const anchor = Date.parse(`${rec.pay_date ?? rec.period_end}T00:00:00`)
    const line = rec.line_items.find((i) => i.line_type === "rsu_vest_value")
    return investments
      .filter((e) => e.event_type === "vest" && accounts.has(e.account_id))
      .filter((e) => Math.abs(Date.parse(e.date.slice(0, 10) + "T00:00:00") - anchor) <= 7 * 86_400_000)
      .map((e) => {
        const value = eventGbp(e)
        const matched = line ? Math.abs(value - Number(line.amount)) <= 1 : false
        return {
          investment_event_id: e.id, account_name: e.account_id, symbol: e.symbol, date: e.date.slice(0, 10),
          quantity: e.quantity, price_per_share: e.price_per_share, currency: e.currency,
          value_gbp: value.toFixed(2), status: matched ? "matched" : "mismatch",
        } satisfies VestCandidate
      })
  }

  // ── Summary ─────────────────────────────────────────────────────

  summary(taxYear: string, profileId: string, investments: InvestmentEvent[]): TaxYearSummary {
    const [start] = ukStart(taxYear)
    const sources = this.listSources(profileId)
    const cgt_link: CgtLink = { start_date: start, end_date: ukEnd(taxYear) }
    return {
      tax_year: taxYear, start_date: start, end_date: ukEnd(taxYear), profile_id: profileId,
      employment: sources.filter((s) => s.source_type === "employment").map((s) => this.employmentSummary(s, taxYear, investments)),
      self_employment: sources.filter((s) => s.source_type === "self_employment").map((s) => this.businessSummary(s, taxYear)),
      interest: taxYear === this.seedTaxYear ? INTEREST[profileId] ?? emptyInterest() : emptyInterest(),
      donations: taxYear === this.seedTaxYear ? DONATIONS[profileId] ?? emptyDonations() : emptyDonations(),
      cgt_link, warnings: [],
    }
  }

  private yearRecords(sourceId: string, taxYear: string): IncomeRecord[] {
    return this.records.filter((r) => r.source_id === sourceId && r.tax_year === taxYear)
  }

  private figure(check: IncomeCheck, types: IncomeLineType[]): number {
    for (const t of types) {
      const f = check.figures.find((x) => x.line_type === t)
      if (f) return toCents(f.amount)
    }
    return 0
  }

  private checkExpenses(check: IncomeCheck): number {
    return check.figures
      .filter((f) => INFO.get(f.line_type)?.source_type === "self_employment" && INFO.get(f.line_type)?.summed && f.line_type !== "sales" && f.line_type !== "refunds")
      .reduce((n, f) => n + toCents(f.amount), 0)
  }

  private employmentSums(recs: IncomeRecord[]): { pay: number; tax: number } {
    const sum = (r: IncomeRecord, lt: IncomeLineType) =>
      r.line_items.filter((i) => i.line_type === lt && i.basis === "period").reduce((n, i) => n + toCents(i.amount), 0)
    const has = (r: IncomeRecord, lt: IncomeLineType) => r.line_items.some((i) => i.line_type === lt && i.basis === "period")
    return {
      pay: recs.reduce((n, r) => n + (has(r, "taxable_pay") ? sum(r, "taxable_pay") : sum(r, "gross_pay")), 0),
      tax: recs.reduce((n, r) => n + sum(r, "paye_tax"), 0),
    }
  }

  private bank(source: IncomeSource, taxYear: string): { mapped: Partial<Record<IncomeLineType, number>>; summary: SelfEmploymentSummary["bank_expenses"] } {
    const mapped: Partial<Record<IncomeLineType, number>> = {}
    const unmapped: SelfEmploymentSummary["bank_expenses"]["unmapped"] = []
    if (taxYear === this.seedTaxYear && source.account_ids.length > 0) {
      for (const o of BANK_OUTFLOWS) {
        const lt = source.expense_category_map[o.category_id]
        if (lt) mapped[lt] = (mapped[lt] ?? 0) + toCents(o.total)
        else unmapped.push(o)
      }
    }
    const total = Object.values(mapped).reduce((n, v) => n + (v ?? 0), 0)
    return { mapped, summary: { mapped_total: fromCents(total), unmapped } }
  }

  private businessSums(source: IncomeSource, recs: IncomeRecord[], taxYear: string): { turnover: number; expenses: number } {
    let turnover = 0
    let expenses = toCents(this.bank(source, taxYear).summary.mapped_total)
    for (const r of recs) {
      for (const i of r.line_items) {
        if (i.basis !== "period") continue
        const info = INFO.get(i.line_type)
        if (!info?.summed) continue
        if (i.line_type === "sales") turnover += toCents(i.amount)
        else if (i.line_type === "refunds") turnover -= toCents(i.amount)
        else expenses += toCents(i.amount)
      }
    }
    return { turnover, expenses }
  }

  private checkStatus(periodGap: boolean, totalGap: boolean, hasAdjustment: boolean): "matched" | "gap" | "adjusted" {
    if (!periodGap) return "matched"
    return hasAdjustment && !totalGap ? "adjusted" : "gap"
  }

  private employmentSummary(source: IncomeSource, taxYear: string, investments: InvestmentEvent[]): EmploymentSourceSummary {
    const recs = this.yearRecords(source.id, taxYear)
    const period = recs.filter((r) => r.kind === "period")
    const total = (lt: IncomeLineType) =>
      sumMoney(recs.flatMap((r) => r.line_items.filter((i) => i.line_type === lt && i.basis === "period").map((i) => i.amount)))
    const all = this.employmentSums(recs)
    const check = this.checks.find((c) => c.source_id === source.id && c.tax_year === taxYear && c.check_type === "p60")
    let checkSummary: EmploymentSourceSummary["check"]
    if (check) {
      const pay = this.figure(check, ["taxable_pay", "gross_pay"])
      const tax = this.figure(check, ["paye_tax"])
      const p = this.employmentSums(period)
      checkSummary = {
        check_id: check.id, pay: fromCents(pay), tax: fromCents(tax),
        pay_difference: fromCents(pay - all.pay), tax_difference: fromCents(tax - all.tax),
        status: this.checkStatus(pay !== p.pay || tax !== p.tax, pay !== all.pay || tax !== all.tax, recs.some((r) => r.kind === "year_end_adjustment")),
      }
    }
    const rsu: RsuVestSummary[] = recs.flatMap((r) =>
      r.line_items.filter((i) => i.line_type === "rsu_vest_value").map((i) => {
        const event = i.investment_event_id ? investments.find((e) => e.id === i.investment_event_id) : undefined
        const value = event ? eventGbp(event) : undefined
        const status: RsuVestSummary["status"] = !event ? "unlinked" : Math.abs((value ?? 0) - Number(i.amount)) <= 1 ? "matched" : "mismatch"
        return {
          record_id: r.id, line_item_id: i.id, vest_value: i.amount, investment_event_id: i.investment_event_id,
          event_value: value === undefined ? undefined : value.toFixed(2), status,
        }
      }),
    )
    return {
      source_id: source.id, name: source.name, paye_reference: source.identifiers.paye_reference,
      record_count: recs.length, months_covered: [...new Set(recs.filter((r) => r.kind === "period").map((r) => (r.pay_date ?? r.period_end).slice(0, 7)))].sort(),
      boxes: { pay: fromCents(all.pay), tax: fromCents(all.tax), student_loan: total("student_loan"), postgraduate_loan: total("postgraduate_loan") },
      shown: {
        gross_pay: total("gross_pay"), employee_ni: total("employee_ni"), employee_pension: total("employee_pension"),
        rsu_vest_value: total("rsu_vest_value"), rsu_tax: total("rsu_tax"), net_pay: total("net_pay"),
      },
      check: checkSummary, rsu,
    }
  }

  private businessSummary(source: IncomeSource, taxYear: string): SelfEmploymentSummary {
    const recs = this.yearRecords(source.id, taxYear)
    const period = recs.filter((r) => r.kind === "period")
    const bank = this.bank(source, taxYear)
    const byType: Partial<Record<IncomeLineType, number>> = { ...bank.mapped }
    const info: Record<string, string> = {}
    for (const r of recs) {
      for (const i of r.line_items) {
        if (i.basis !== "period") continue
        const meta = INFO.get(i.line_type)
        if (i.line_type.startsWith("costing_")) info[i.line_type] = sumMoney([info[i.line_type], i.amount])
        else if (meta?.summed && meta.source_type === "self_employment" && i.line_type !== "sales" && i.line_type !== "refunds") {
          byType[i.line_type] = (byType[i.line_type] ?? 0) + toCents(i.amount)
        }
      }
    }
    const all = this.businessSums(source, recs, taxYear)
    const expensesByType: Record<string, string> = {}
    for (const [k, v] of Object.entries(byType)) if (v) expensesByType[k] = fromCents(v)
    const check = this.checks.find((c) => c.source_id === source.id && c.tax_year === taxYear && c.check_type === "annual_accounts")
    let checkSummary: SelfEmploymentSummary["check"]
    if (check) {
      const turnover = this.figure(check, ["sales"]) - this.figure(check, ["refunds"])
      const expenses = this.checkExpenses(check)
      const p = this.businessSums(source, period, taxYear)
      checkSummary = {
        check_id: check.id, turnover: fromCents(turnover), expenses: fromCents(expenses),
        turnover_difference: fromCents(turnover - all.turnover), expenses_difference: fromCents(expenses - all.expenses),
        status: this.checkStatus(turnover !== p.turnover || expenses !== p.expenses, turnover !== all.turnover || expenses !== all.expenses, recs.some((r) => r.kind === "year_end_adjustment")),
      }
    }
    return {
      source_id: source.id, name: source.name,
      months_covered: [...new Set(period.map((r) => r.period_end.slice(0, 7)))].sort(),
      turnover: fromCents(all.turnover), expenses_by_line_type: expensesByType, total_expenses: fromCents(all.expenses),
      net_profit: fromCents(all.turnover - all.expenses),
      trading_allowance_better: all.turnover > 0 && all.expenses < TRADING_ALLOWANCE_CENTS,
      bank_expenses: bank.summary, info, check: checkSummary,
    }
  }
}

/** The mock vest event's GBP value at the fixed mock rate. */
export function eventGbp(e: InvestmentEvent): number {
  const rate = e.currency === "GBP" ? 1 : USD_TO_GBP
  return Number(e.quantity) * Number(e.price_per_share) * rate
}

function ukStart(taxYear: string): [string, string] {
  const y = Number(taxYear.slice(0, 4))
  return [ymd(y, 4, 6), ymd(y + 1, 4, 5)]
}

function ukEnd(taxYear: string): string {
  return ukStart(taxYear)[1]
}

function ukMonthBefore(date: string): string {
  const [y, m] = date.split("-").map(Number)
  return m === 1 ? `${y - 1}-12` : `${y}-${String(m - 1).padStart(2, "0")}`
}

function emptyInterest(): InterestSummary {
  return { taxable_total: "0.00", non_taxable_total: "0.00", by_account: [] }
}

function emptyDonations(): DonationSummary {
  return { gift_aid_total: "0.00", non_gift_aid_total: "0.00" }
}
