import type { IncomeLineItem } from "@/bindings/IncomeLineItem"
import type { IncomeLineItemInput } from "@/bindings/IncomeLineItemInput"
import type { IncomeLineType } from "@/bindings/IncomeLineType"
import type { IncomeRecord } from "@/bindings/IncomeRecord"
import type { IncomeSourceType } from "@/bindings/IncomeSourceType"
import type { LineTypeInfo } from "@/bindings/LineTypeInfo"
import { sumMoney } from "@/lib/money"

/** Columns every table of a source type always shows, even when empty. */
const DEFAULT_COLUMNS: Partial<Record<IncomeSourceType, IncomeLineType[]>> = {
  employment: ["taxable_pay", "paye_tax"],
  self_employment: ["sales"],
}

export interface TableColumns {
  /** Lines that feed a form box, in catalogue order. */
  box: LineTypeInfo[]
  /** Lines shown for information, in catalogue order. */
  shown: LineTypeInfo[]
}

function periodAmount(record: IncomeRecord, lineType: IncomeLineType): string | null {
  const items = record.line_items.filter((i) => i.line_type === lineType && i.basis === "period")
  return items.length === 0 ? null : sumMoney(items.map((i) => i.amount))
}

/**
 * Columns for a source type's monthly table, driven by the line-type
 * catalogue: the type's defaults plus every line type some record carries.
 * Shop-app costings are information only and never get a column.
 */
export function visibleColumns(
  catalogue: LineTypeInfo[],
  sourceType: IncomeSourceType,
  records: IncomeRecord[],
): TableColumns {
  const defaults = DEFAULT_COLUMNS[sourceType] ?? []
  const used = new Set<IncomeLineType>(defaults)
  for (const r of records) {
    for (const item of r.line_items) {
      if (item.basis === "period" && Number(item.amount) !== 0) used.add(item.line_type)
    }
  }
  const columns = catalogue.filter(
    (c) => c.source_type === sourceType && used.has(c.line_type) && !c.line_type.startsWith("costing_"),
  )
  return { box: columns.filter((c) => c.summed), shown: columns.filter((c) => !c.summed) }
}

/** One record's amount for a line type (period basis only; year-to-date is never summed), or null when absent. */
export function recordCell(record: IncomeRecord, lineType: IncomeLineType): string | null {
  return periodAmount(record, lineType)
}

/** Records in table order: pay date (else period end), adjustments last. */
export function sortRecords(records: IncomeRecord[]): IncomeRecord[] {
  const key = (r: IncomeRecord) => r.pay_date ?? r.period_end
  return records.slice().sort((a, b) => {
    const adj = Number(a.kind === "year_end_adjustment") - Number(b.kind === "year_end_adjustment")
    return adj !== 0 ? adj : key(a).localeCompare(key(b))
  })
}

/** Column totals over the given records, one entry per line type that appears. */
export function columnTotals(
  records: IncomeRecord[],
  lineTypes: IncomeLineType[],
): Partial<Record<IncomeLineType, string>> {
  const totals: Partial<Record<IncomeLineType, string>> = {}
  for (const lt of lineTypes) {
    const cells = records.map((r) => periodAmount(r, lt))
    if (cells.some((c) => c !== null)) totals[lt] = sumMoney(cells)
  }
  return totals
}

/** Employment box 1 over records: taxable pay, else gross pay, per record (the backend rule). */
export function employmentPayBox(records: IncomeRecord[]): string {
  return sumMoney(
    records.map((r) => periodAmount(r, "taxable_pay") ?? periodAmount(r, "gross_pay")),
  )
}

/** Month label for a record row, `YYYY-MM` of the pay date (employment) or period end. */
export function recordMonth(record: IncomeRecord): string {
  return (record.pay_date ?? record.period_end).slice(0, 7)
}

/** The twelve `YYYY-MM` months a tax year ("2025-26") touches, April first. */
export function taxYearMonths(taxYear: string): string[] {
  const start = Number(taxYear.slice(0, 4))
  if (!Number.isFinite(start)) return []
  return Array.from({ length: 12 }, (_, i) => {
    const month = ((3 + i) % 12) + 1
    const year = start + (3 + i >= 12 ? 1 : 0)
    return `${year}-${String(month).padStart(2, "0")}`
  })
}

/** Months of the tax year with no record, given the summary's `months_covered`. */
export function missingMonths(taxYear: string, covered: string[]): string[] {
  const have = new Set(covered)
  return taxYearMonths(taxYear).filter((m) => !have.has(m))
}

/** Edit-form input for an existing line item, keeping its link and label. */
export function lineItemToInput(item: IncomeLineItem): IncomeLineItemInput {
  return {
    line_type: item.line_type,
    amount: item.amount,
    currency: item.currency,
    basis: item.basis,
    label: item.label,
    investment_event_id: item.investment_event_id,
  }
}

/**
 * Price per share that makes a vest event's GBP value equal the value taxed
 * as pay. The vest-date rate is implied by the candidate (`valueGbp` over
 * quantity times price), so the result stays in the event's own currency.
 * Null when the event has no usable value.
 */
export function alignedPricePerShare(
  pricePerShare: string,
  valueGbp: string,
  payslipValue: string,
): string | null {
  const price = Number(pricePerShare)
  const current = Number(valueGbp)
  const target = Number(payslipValue)
  if (!Number.isFinite(price) || !Number.isFinite(current) || !Number.isFinite(target)) return null
  if (current <= 0 || price <= 0 || target <= 0) return null
  return ((price * target) / current).toFixed(4)
}
