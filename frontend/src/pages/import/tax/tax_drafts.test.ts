import { describe, expect, it } from "vitest"
import type { TaxDocumentDraft } from "@/bindings/TaxDocumentDraft"
import type { TaxDocumentIngestionResult } from "@/bindings/TaxDocumentIngestionResult"
import type { TaxDocumentPreviewRow } from "@/bindings/TaxDocumentPreviewRow"
import {
  applyCommitErrors,
  buildCommitPayload,
  invalidAmountTypes,
  isValidAmount,
  rowsWithClashingLinks,
  rowsWithInvalidAmounts,
  takenTransactionIds,
  taxYearMismatchWarning,
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

describe("isValidAmount", () => {
  it("accepts non-negative decimals", () => {
    for (const v of ["0", "12", "12.50", " 3.1 "]) expect(isValidAmount(v)).toBe(true)
  })
  it("rejects blank, minus, negatives and half-typed numbers", () => {
    for (const v of ["", "-", "-5", "-5.00", ".", "5.", ".5", "1e3", "abc"]) {
      expect(isValidAmount(v)).toBe(false)
    }
  })
})

describe("invalid amounts", () => {
  const bad = draft({
    line_items: [
      { line_type: "gross_pay", amount: "-", basis: "period" },
      { line_type: "paye_tax", amount: "10.00", basis: "period" },
      { line_type: "employee_ni", amount: "-4.00", basis: "period" },
      { line_type: "gross_pay", amount: "oops", basis: "year_to_date" },
    ],
  })
  it("lists period items that are not non-negative decimals and ignores year to date", () => {
    expect(invalidAmountTypes(bad)).toEqual(["gross_pay", "employee_ni"])
  })
  it("flags only kept, sendable rows", () => {
    const rows = [row(0, "new", bad), row(1, "new", bad), row(2, "error", bad), row(3, "new")]
    expect(rowsWithInvalidAmounts(rows, new Set([0, 2, 3]))).toEqual([0])
  })
})

describe("bank credit de-dup", () => {
  const linked = (index: number, id: string | undefined, status: TaxDocumentPreviewRow["status"] = "new") =>
    row(index, status, draft({ transaction_id: id }))
  const rows = [linked(0, "tx_a"), linked(1, "tx_b"), linked(2, "tx_a"), linked(3, undefined)]

  it("hides credits linked on other kept rows, not the row's own", () => {
    const taken = takenTransactionIds(rows, new Set([0, 1, 2, 3]), 1)
    expect([...taken].sort()).toEqual(["tx_a"])
    expect([...takenTransactionIds(rows, new Set([0, 1]), 0)]).toEqual(["tx_b"])
  })
  it("ignores unkept and error rows", () => {
    const withError = [linked(0, "tx_a", "error"), linked(1, "tx_b")]
    expect(takenTransactionIds(withError, new Set([0, 1]), 1).size).toBe(0)
    expect(takenTransactionIds(rows, new Set([1]), 3).has("tx_a")).toBe(false)
  })
  it("reports the later row when two kept rows share a credit", () => {
    expect(rowsWithClashingLinks(rows, new Set([0, 1, 2, 3]))).toEqual([2])
    expect(rowsWithClashingLinks(rows, new Set([0, 1, 3]))).toEqual([])
  })
})

describe("applyCommitErrors", () => {
  it("maps error positions back to rows and unticks only the saved ones", () => {
    // Rows 0, 2, 3 were sent; the backend rejected its documents[1], which is row 2.
    const out = applyCommitErrors([0, 2, 3], [{ index: 1, reason: "already linked" }], new Set([0, 2, 3, 5]))
    expect(out.rejected).toEqual([{ rowIndex: 2, reason: "already linked" }])
    expect(out.savedRows).toEqual([0, 3])
    expect([...out.kept].sort()).toEqual([2, 5])
  })
  it("falls back to the raw position when it is out of range", () => {
    const out = applyCommitErrors([4], [{ index: 9, reason: "bad" }], new Set([4]))
    expect(out.rejected[0].rowIndex).toBe(9)
    expect(out.savedRows).toEqual([4])
  })
  it("does not mutate the kept set", () => {
    const kept = new Set([0])
    applyCommitErrors([0], [], kept)
    expect([...kept]).toEqual([0])
  })
})

describe("taxYearMismatchWarning", () => {
  it("is null when the year matches the pay date", () => {
    expect(taxYearMismatchWarning(draft({ pay_date: "2025-12-19" }))).toBeNull()
  })
  it("warns when the year disagrees", () => {
    expect(taxYearMismatchWarning(draft({ pay_date: "2026-04-10" }))).toMatch(/2026-27/)
  })
  it("skips P60s and drafts without a pay date", () => {
    expect(taxYearMismatchWarning(draft({ document_type: "annual_certificate", pay_date: "2026-04-10" }))).toBeNull()
    expect(taxYearMismatchWarning(draft())).toBeNull()
  })
})
