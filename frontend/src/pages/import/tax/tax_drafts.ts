import type { IncomeLineType } from "@/bindings/IncomeLineType"
import type { TaxDocumentDraft } from "@/bindings/TaxDocumentDraft"
import type { TaxDocumentIngestionResult } from "@/bindings/TaxDocumentIngestionResult"
import type { TaxDocumentPreviewRow } from "@/bindings/TaxDocumentPreviewRow"
import type { TaxDocumentsImportPayload } from "@/bindings/TaxDocumentsImportPayload"
import type { TaxDocumentImportError } from "@/bindings/TaxDocumentImportError"
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

/** A non-negative decimal as the backend reads it: digits, with an optional fraction that has digits. */
const AMOUNT_RE = /^\d+(\.\d+)?$/

/** True when `value` is a whole non-negative decimal. A blank value is not valid; callers treat blank as "no item". */
export function isValidAmount(value: string): boolean {
  return AMOUNT_RE.test(value.trim())
}

/** The period-basis line types of `draft` whose amount is not a non-negative decimal. */
export function invalidAmountTypes(draft: TaxDocumentDraft): IncomeLineType[] {
  return draft.line_items
    .filter((i) => i.basis === "period" && !isValidAmount(i.amount))
    .map((i) => i.line_type)
}

/** Row indices of kept, sendable rows that have at least one invalid period amount. */
export function rowsWithInvalidAmounts(
  rows: TaxDocumentPreviewRow[],
  kept: ReadonlySet<number>,
): number[] {
  return rows
    .filter((r) => kept.has(r.index) && r.status !== "error" && invalidAmountTypes(r.draft).length > 0)
    .map((r) => r.index)
}

/**
 * Bank credits already linked to another kept row, so the picker can hide them.
 * `exceptIndex` is the row being edited, whose own link does not count.
 */
export function takenTransactionIds(
  rows: TaxDocumentPreviewRow[],
  kept: ReadonlySet<number>,
  exceptIndex: number,
): Set<string> {
  const taken = new Set<string>()
  for (const r of rows) {
    const id = r.draft.transaction_id
    if (id && r.index !== exceptIndex && kept.has(r.index) && r.status !== "error") taken.add(id)
  }
  return taken
}

/** Row indices of kept, sendable rows that share a bank credit with an earlier kept row. */
export function rowsWithClashingLinks(
  rows: TaxDocumentPreviewRow[],
  kept: ReadonlySet<number>,
): number[] {
  const seen = new Set<string>()
  const clashing: number[] = []
  for (const r of rows) {
    const id = r.draft.transaction_id
    if (!id || !kept.has(r.index) || r.status === "error") continue
    if (seen.has(id)) clashing.push(r.index)
    else seen.add(id)
  }
  return clashing
}

/** A warning when the draft's tax year does not match the year its pay date falls in, else null. */
export function taxYearMismatchWarning(draft: TaxDocumentDraft): string | null {
  if (draft.document_type !== "payslip" || !draft.pay_date) return null
  const expected = taxYearForDate(draft.pay_date)
  if (expected === draft.tax_year) return null
  return `Pay date falls in tax year ${expected}, but this row says ${draft.tax_year}.`
}

export interface CommitOutcome {
  /** Row indices that were written: everything sent that the backend did not reject. */
  savedRows: number[]
  /** Rejected rows with the backend's reason, by preview row index. */
  rejected: Array<{ rowIndex: number; reason: string }>
  /** The kept set after the save: saved rows are unticked so a second save resends only the rejected ones. */
  kept: Set<number>
}

/**
 * Maps the backend's per-row errors (positions in the documents it received)
 * back to preview rows. The backend saves every row it did not reject, so the
 * rejected rows stay ticked and the rest are unticked.
 */
export function applyCommitErrors(
  sentRowIndices: number[],
  errors: TaxDocumentImportError[],
  kept: ReadonlySet<number>,
): CommitOutcome {
  const rejected = errors.map((e) => ({ rowIndex: sentRowIndices[e.index] ?? e.index, reason: e.reason }))
  const rejectedSet = new Set(rejected.map((r) => r.rowIndex))
  const savedRows = sentRowIndices.filter((i) => !rejectedSet.has(i))
  const next = new Set(kept)
  for (const i of savedRows) next.delete(i)
  return { savedRows, rejected, kept: next }
}
