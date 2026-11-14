import { describe, expect, it } from "vitest"
import type { TaxDocumentDraft } from "@/bindings/TaxDocumentDraft"
import type { TaxDocumentIngestionResult } from "@/bindings/TaxDocumentIngestionResult"
import type { TaxDocumentPreviewRow } from "@/bindings/TaxDocumentPreviewRow"
import {
  buildCommitPayload,
  defaultKept,
  lineTypeLabel,
  periodAmount,
  setPeriodAmount,
  taxYearForDate,
} from "./tax_drafts"

function draft(over: Partial<TaxDocumentDraft> = {}): TaxDocumentDraft {
  return {
    document_type: "payslip",
    source_name: "Example Widgets Ltd",
    tax_year: "2025-26",
    currency: "GBP",
    line_items: [
      { line_type: "gross_pay", amount: "3000.00", basis: "period" },
      { line_type: "gross_pay", amount: "9000.00", basis: "year_to_date" },
    ],
    document_ids: ["d1"],
    ...over,
  }
}

function row(index: number, status: TaxDocumentPreviewRow["status"], d = draft()): TaxDocumentPreviewRow {
  return {
    index,
    status,
    row_confidence: 0.9,
    source_document_ids: ["d1"],
    warnings: [],
    net_pay_candidates: [],
    draft: d,
  }
}

describe("periodAmount", () => {
  it("reads the period item, not year to date", () => {
    expect(periodAmount(draft(), "gross_pay")).toBe("3000.00")
  })
  it("is null when only a year-to-date item exists", () => {
    const d = draft({ line_items: [{ line_type: "paye_tax", amount: "5.00", basis: "year_to_date" }] })
    expect(periodAmount(d, "paye_tax")).toBeNull()
  })
})

describe("setPeriodAmount", () => {
  it("edits the period item and leaves year to date alone", () => {
    const out = setPeriodAmount(draft(), "gross_pay", "3100.00")
    expect(out.line_items).toEqual([
      { line_type: "gross_pay", amount: "3100.00", basis: "period" },
      { line_type: "gross_pay", amount: "9000.00", basis: "year_to_date" },
    ])
  })
  it("inserts a period item next to an existing year-to-date one", () => {
    const d = draft({ line_items: [{ line_type: "paye_tax", amount: "50.00", basis: "year_to_date" }] })
    const out = setPeriodAmount(d, "paye_tax", "10.00")
    expect(out.line_items).toHaveLength(2)
    expect(out.line_items[0]).toEqual({ line_type: "paye_tax", amount: "50.00", basis: "year_to_date" })
    expect(out.line_items[1]).toEqual({ line_type: "paye_tax", amount: "10.00", basis: "period" })
  })
  it("removes the period item on a blank value", () => {
    const out = setPeriodAmount(draft(), "gross_pay", " ")
    expect(out.line_items).toEqual([{ line_type: "gross_pay", amount: "9000.00", basis: "year_to_date" }])
  })
  it("does not mutate the input", () => {
    const d = draft()
    setPeriodAmount(d, "gross_pay", "1.00")
    expect(periodAmount(d, "gross_pay")).toBe("3000.00")
  })
})

describe("defaultKept", () => {
  it("is true only for new rows", () => {
    expect(defaultKept(row(0, "new"))).toBe(true)
    expect(defaultKept(row(1, "duplicate"))).toBe(false)
    expect(defaultKept(row(2, "error"))).toBe(false)
  })
})

describe("buildCommitPayload", () => {
  const result = { profile_id: "alex" } as TaxDocumentIngestionResult
  it("keeps drafts in preview order and drops unkept rows", () => {
    const rows = [
      row(0, "new", draft({ source_name: "A" })),
      row(1, "new", draft({ source_name: "B" })),
      row(2, "new", draft({ source_name: "C" })),
    ]
    const out = buildCommitPayload(result, rows, new Set([2, 0]))
    expect(out.profile_id).toBe("alex")
    expect(out.documents.map((d) => d.source_name)).toEqual(["A", "C"])
  })
  it("never sends error rows", () => {
    const out = buildCommitPayload(result, [row(0, "error")], new Set([0]))
    expect(out.documents).toEqual([])
  })
})

describe("taxYearForDate", () => {
  it("5 April is still the old year", () => {
    expect(taxYearForDate("2026-04-05")).toBe("2025-26")
  })
  it("6 April starts the next year", () => {
    expect(taxYearForDate("2026-04-06")).toBe("2026-27")
  })
})

describe("lineTypeLabel", () => {
  it("uses the reader name, else a humanised code", () => {
    expect(lineTypeLabel("employee_ni")).toBe("NI")
    expect(lineTypeLabel("rent_received")).toBe("Rent received")
  })
})
