import type { IncomeLineType } from "@/bindings/IncomeLineType"
import type { TaxDocumentDraft } from "@/bindings/TaxDocumentDraft"
import type { TaxDocumentIngestionResult } from "@/bindings/TaxDocumentIngestionResult"
import type { TaxDocumentPreviewRow } from "@/bindings/TaxDocumentPreviewRow"
import type { TaxDocumentsImportPayload } from "@/bindings/TaxDocumentsImportPayload"
import { ukTaxYearForDate } from "@/api/cgt_filter_params"

/** Reader-facing names for the line types a payslip or P60 carries. */
const LINE_LABELS: Partial<Record<IncomeLineType, string>> = {
  gross_pay: "Gross pay",
  taxable_pay: "Taxable pay",
  paye_tax: "Tax",
  employee_ni: "NI",
  student_loan: "Student loan",
  postgraduate_loan: "Postgraduate loan",
  employee_pension: "Pension",
  rsu_vest_value: "RSU vest value",
  rsu_tax: "RSU tax",
  net_pay: "Net pay",
  other_payment: "Other payment",
  other_deduction: "Other deduction",
}

export function lineTypeLabel(lineType: IncomeLineType): string {
  if (LINE_LABELS[lineType]) return LINE_LABELS[lineType]!
  const spaced = lineType.replace(/_/g, " ")
  return spaced.charAt(0).toUpperCase() + spaced.slice(1)
}

/** The draft's period-basis amount for a line type, or null when it has none. Year-to-date items never count. */
export function periodAmount(draft: TaxDocumentDraft, code: IncomeLineType): string | null {
  const item = draft.line_items.find((i) => i.line_type === code && i.basis === "period")
  return item ? item.amount : null
}

/**
 * Returns a draft with the period amount for `code` set to `value`: edits the
 * existing period item, or inserts one. A blank value removes the item. Items
 * with any other basis are left untouched.
 */
export function setPeriodAmount(
  draft: TaxDocumentDraft,
  code: IncomeLineType,
  value: string,
): TaxDocumentDraft {
  const trimmed = value.trim()
  const idx = draft.line_items.findIndex((i) => i.line_type === code && i.basis === "period")
  if (trimmed === "") {
    if (idx === -1) return draft
    return { ...draft, line_items: draft.line_items.filter((_, i) => i !== idx) }
  }
  if (idx === -1) {
    return {
      ...draft,
      line_items: [...draft.line_items, { line_type: code, amount: trimmed, basis: "period" }],
    }
  }
  return {
    ...draft,
    line_items: draft.line_items.map((item, i) => (i === idx ? { ...item, amount: trimmed } : item)),
  }
}

/** Only rows the backend marked new start out kept; duplicates and errors start skipped. */
export function defaultKept(row: TaxDocumentPreviewRow): boolean {
  return row.status === "new"
}

/**
 * The commit body: the kept rows' drafts in preview order. `rows` carries the
 * user's edits (same order and `index` values as `result.rows`); `kept` holds
 * row indices. Error rows are never sent.
 */
export function buildCommitPayload(
  result: TaxDocumentIngestionResult,
  rows: TaxDocumentPreviewRow[],
  kept: ReadonlySet<number>,
): TaxDocumentsImportPayload {
  return {
    profile_id: result.profile_id,
    documents: rows.filter((r) => kept.has(r.index) && r.status !== "error").map((r) => r.draft),
  }
}

/** UK tax year (`2025-26`) containing an ISO `YYYY-MM-DD` date, read as a local date. */
export function taxYearForDate(iso: string): string {
  const [y, m, d] = iso.slice(0, 10).split("-").map(Number)
  return ukTaxYearForDate(new Date(y, m - 1, d))
}
