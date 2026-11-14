import { describe, expect, it } from "vitest"
import type { IncomeLineType } from "@/bindings/IncomeLineType"
import type { IncomeRecord } from "@/bindings/IncomeRecord"
import type { LineTypeInfo } from "@/bindings/LineTypeInfo"
import {
  alignedPricePerShare, columnTotals, employmentPayBox, missingMonths, recordCell,
  recordMonth, sortRecords, taxYearMonths, visibleColumns,
} from "./tax_helpers"

function info(line_type: IncomeLineType, source_type: LineTypeInfo["source_type"], summed: boolean): LineTypeInfo {
  return { line_type, source_type, label: line_type, summed, sign: 1, form: "", box_ref: "", note: "" }
}

const CATALOGUE: LineTypeInfo[] = [
  info("taxable_pay", "employment", true),
  info("gross_pay", "employment", true),
  info("paye_tax", "employment", true),
  info("employee_ni", "employment", false),
  info("sales", "self_employment", true),
  info("platform_fees", "self_employment", true),
  info("costing_materials", "self_employment", false),
]

function record(
  id: string, period_end: string, lines: [IncomeLineType, string, ("period" | "year_to_date")?][],
  extra: Partial<IncomeRecord> = {},
): IncomeRecord {
  return {
    id, source_id: "s1", kind: "period", period_start: period_end.slice(0, 8) + "01", period_end,
    tax_year: "2025-26", currency: "GBP", transaction_ids: [], document_ids: [], origin: "manual",
    line_items: lines.map(([line_type, amount, basis], i) => ({
      id: `${id}_${i}`, line_type, amount, currency: "GBP", basis: basis ?? "period",
    })),
    created_at: "", updated_at: "", ...extra,
  }
}

describe("visibleColumns", () => {
  it("shows the defaults for an empty employment source", () => {
    const cols = visibleColumns(CATALOGUE, "employment", [])
    expect(cols.box.map((c) => c.line_type)).toEqual(["taxable_pay", "paye_tax"])
    expect(cols.shown).toEqual([])
  })

  it("adds carried line types in catalogue order, split by summed flag", () => {
    const rs = [record("a", "2025-05-31", [["gross_pay", "10"], ["employee_ni", "1"], ["paye_tax", "2"]])]
    const cols = visibleColumns(CATALOGUE, "employment", rs)
    expect(cols.box.map((c) => c.line_type)).toEqual(["taxable_pay", "gross_pay", "paye_tax"])
    expect(cols.shown.map((c) => c.line_type)).toEqual(["employee_ni"])
  })

  it("ignores year-to-date lines and never shows costings or other source types", () => {
    const rs = [record("a", "2025-05-31", [["employee_ni", "9", "year_to_date"], ["costing_materials", "4"], ["sales", "5"]])]
    const cols = visibleColumns(CATALOGUE, "employment", rs)
    expect(cols.shown).toEqual([])
    const biz = visibleColumns(CATALOGUE, "self_employment", rs)
    expect(biz.box.map((c) => c.line_type)).toEqual(["sales"])
    expect(biz.shown).toEqual([])
  })
})

describe("record cells and totals", () => {
  const rs = [
    record("a", "2025-05-31", [["taxable_pay", "100.10"], ["paye_tax", "10"], ["paye_tax", "5.05", "year_to_date"]]),
    record("b", "2025-06-30", [["taxable_pay", "200.20"]]),
  ]

  it("reads period amounts only", () => {
    expect(recordCell(rs[0], "paye_tax")).toBe("10.00")
    expect(recordCell(rs[1], "paye_tax")).toBeNull()
  })

  it("sums columns exactly and omits absent line types", () => {
    expect(columnTotals(rs, ["taxable_pay", "paye_tax", "student_loan"])).toEqual({
      taxable_pay: "300.30",
      paye_tax: "10.00",
    })
  })

  it("box 1 prefers taxable pay per record, falling back to gross", () => {
    const mixed = [
      record("a", "2025-05-31", [["taxable_pay", "90"], ["gross_pay", "100"]]),
      record("b", "2025-06-30", [["gross_pay", "50"]]),
    ]
    expect(employmentPayBox(mixed)).toBe("140.00")
  })
})

describe("ordering and months", () => {
  it("sorts by pay date then period end with adjustments last", () => {
    const adj = record("adj", "2026-04-05", [["taxable_pay", "1"]], { kind: "year_end_adjustment" })
    const late = record("late", "2025-05-31", [], { pay_date: "2025-07-01" })
    const early = record("early", "2025-06-30", [])
    expect(sortRecords([adj, late, early]).map((r) => r.id)).toEqual(["early", "late", "adj"])
    expect(recordMonth(late)).toBe("2025-07")
  })

  it("lists the twelve months of a tax year and what is missing", () => {
    const months = taxYearMonths("2025-26")
    expect(months[0]).toBe("2025-04")
    expect(months[8]).toBe("2025-12")
    expect(months[9]).toBe("2026-01")
    expect(months[11]).toBe("2026-03")
    expect(missingMonths("2025-26", months.slice(0, 10))).toEqual(["2026-02", "2026-03"])
  })
})

describe("alignedPricePerShare", () => {
  it("scales the price by payslip value over event value", () => {
    expect(alignedPricePerShare("150.0000", "3750.00", "3800.00")).toBe("152.0000")
  })

  it("keeps the event currency: the implied rate cancels out", () => {
    // 25 shares at 150 USD valued 3000 GBP (rate 0.8); payslip taxed 3150 GBP.
    expect(alignedPricePerShare("150.0000", "3000.00", "3150.00")).toBe("157.5000")
  })

  it("returns null when there is nothing to scale from", () => {
    expect(alignedPricePerShare("150", "0", "3000")).toBeNull()
    expect(alignedPricePerShare("abc", "10", "10")).toBeNull()
  })
})
