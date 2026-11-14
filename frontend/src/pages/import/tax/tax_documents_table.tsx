import { Fragment, useState } from "react"
import { AlertTriangle, ChevronRight } from "lucide-react"
import { Badge } from "@/components/ui/badge"
import { Checkbox } from "@/components/ui/checkbox"
import { Input } from "@/components/ui/input"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { SourceChips, type SourceDocMeta } from "@/components/source_chips"
import { cn, formatDate } from "@/lib/utils"
import type { IncomeLineType } from "@/bindings/IncomeLineType"
import type { TaxDocumentDraft } from "@/bindings/TaxDocumentDraft"
import type { TaxDocumentPreviewRow } from "@/bindings/TaxDocumentPreviewRow"
import { DuplicateHint, StatusBadge } from "../preview/status_badge"
import { DecimalCell } from "../preview/editors"
import { NetPayLinkCell } from "./net_pay_link_cell"
import {
  invalidAmountTypes,
  lineTypeLabel,
  periodAmount,
  setPeriodAmount,
  takenTransactionIds,
  taxYearMismatchWarning,
} from "./tax_drafts"

const AMOUNT_COLUMNS: Array<{ code: IncomeLineType; header: string }> = [
  { code: "gross_pay", header: "Gross pay" },
  { code: "taxable_pay", header: "Taxable pay" },
  { code: "paye_tax", header: "Tax" },
  { code: "employee_ni", header: "NI" },
  { code: "student_loan", header: "Student loan" },
  { code: "net_pay", header: "Net pay" },
]

interface Props {
  /** Rows with the user's edits already applied to `draft`. */
  rows: TaxDocumentPreviewRow[]
  kept: ReadonlySet<number>
  onToggleKept: (rowIndex: number) => void
  onDraftChange: (position: number, draft: TaxDocumentDraft) => void
  knownSources: string[]
  docs: Map<string, SourceDocMeta>
  disabled?: boolean
}

function dateLabel(draft: TaxDocumentDraft): string {
  if (draft.pay_date) return formatDate(draft.pay_date)
  if (draft.period_end) return formatDate(draft.period_end)
  return "—"
}

function periodLabel(draft: TaxDocumentDraft): string | null {
  if (draft.period_start && draft.period_end) {
    return `${formatDate(draft.period_start)} to ${formatDate(draft.period_end)}`
  }
  return null
}

export function TaxDocumentsTable({
  rows,
  kept,
  onToggleKept,
  onDraftChange,
  knownSources,
  docs,
  disabled,
}: Props) {
  const [expanded, setExpanded] = useState<Set<number>>(new Set())
  const toggleExpanded = (index: number) =>
    setExpanded((prev) => {
      const next = new Set(prev)
      if (next.has(index)) next.delete(index)
      else next.add(index)
      return next
    })

  return (
    <div className="overflow-x-auto rounded-lg border">
      <datalist id="tax-known-sources">
        {knownSources.map((s) => (
          <option key={s} value={s} />
        ))}
      </datalist>
      <Table className="min-w-[78rem]">
        <TableHeader>
          <TableRow>
            <TableHead className="w-8" />
            <TableHead className="w-8">Keep</TableHead>
            <TableHead>Type</TableHead>
            <TableHead>Employer</TableHead>
            <TableHead>Tax year</TableHead>
            <TableHead>Pay date</TableHead>
            {AMOUNT_COLUMNS.map((c) => (
              <TableHead key={c.code} className="text-right">
                {c.header}
              </TableHead>
            ))}
            <TableHead>Status</TableHead>
            <TableHead>Bank credit</TableHead>
            <TableHead>Source</TableHead>
            <TableHead className="w-8" />
          </TableRow>
        </TableHeader>
        <TableBody>
          {rows.map((row, pos) => {
            const d = row.draft
            const isError = row.status === "error"
            const locked = disabled || isError
            const isKept = kept.has(row.index)
            const isOpen = expanded.has(row.index)
            const yearWarning = taxYearMismatchWarning(d)
            const notes = [
              ...(row.error_reason ? [row.error_reason] : []),
              ...row.warnings,
              ...(yearWarning ? [yearWarning] : []),
            ]
            const badAmounts = invalidAmountTypes(d)
            const rowNo = row.index + 1
            const period = periodLabel(d)
            return (
              <Fragment key={row.index}>
                <TableRow data-testid="tax-row" className={cn(!isKept && "opacity-60")}>
                  <TableCell>
                    <button
                      type="button"
                      onClick={() => toggleExpanded(row.index)}
                      aria-expanded={isOpen}
                      aria-label={isOpen ? "Hide line items" : "Show line items"}
                      className="rounded p-0.5 text-muted-foreground hover:text-foreground"
                    >
                      <ChevronRight className={cn("h-4 w-4 transition-transform", isOpen && "rotate-90")} />
                    </button>
                  </TableCell>
                  <TableCell>
                    <Checkbox
                      checked={isKept && !isError}
                      onCheckedChange={() => onToggleKept(row.index)}
                      disabled={locked}
                      aria-label={`Save row ${row.index + 1}`}
                    />
                  </TableCell>
                  <TableCell>
                    <Badge variant="outline">
                      {d.document_type === "annual_certificate" ? "P60" : "Payslip"}
                    </Badge>
                  </TableCell>
                  <TableCell>
                    <Input
                      list="tax-known-sources"
                      value={d.source_name}
                      onChange={(e) => onDraftChange(pos, { ...d, source_name: e.target.value })}
                      disabled={locked}
                      aria-label={`Employer, row ${rowNo}`}
                      className="h-7 min-w-40 px-1.5 py-0.5 text-xs"
                    />
                  </TableCell>
                  <TableCell className="text-xs tabular-nums">{d.tax_year}</TableCell>
                  <TableCell className="text-xs tabular-nums">
                    <div>{dateLabel(d)}</div>
                    {period && <div className="text-[10px] text-muted-foreground">{period}</div>}
                  </TableCell>
                  {AMOUNT_COLUMNS.map((c) => (
                    <TableCell key={c.code} className="min-w-24">
                      <DecimalCell
                        value={periodAmount(d, c.code) ?? ""}
                        onChange={(v) => onDraftChange(pos, setPeriodAmount(d, c.code, v))}
                        disabled={locked}
                        nonNegative
                        invalid={badAmounts.includes(c.code)}
                        ariaLabel={`${c.header}, row ${rowNo}`}
                      />
                    </TableCell>
                  ))}
                  <TableCell>
                    <div className="flex items-center gap-1">
                      <StatusBadge status={row.status} errorMessage={row.error_reason} />
                      <DuplicateHint sameKeyAsRow={row.same_key_as_row} />
                    </div>
                  </TableCell>
                  <TableCell className="min-w-44">
                    {d.document_type === "payslip" && !isError ? (
                      <NetPayLinkCell
                        candidates={row.net_pay_candidates}
                        value={d.transaction_id}
                        onChange={(id) => onDraftChange(pos, { ...d, transaction_id: id })}
                        takenIds={takenTransactionIds(rows, kept, row.index)}
                        disabled={disabled}
                      />
                    ) : (
                      <span className="text-xs text-muted-foreground">—</span>
                    )}
                  </TableCell>
                  <TableCell>
                    <SourceChips documentIds={row.source_document_ids} docs={docs} />
                  </TableCell>
                  <TableCell>
                    {notes.length > 0 && (
                      <Tooltip>
                        <TooltipTrigger
                          render={
                            <button
                              type="button"
                              className="cursor-help rounded"
                              aria-label={`Warnings, row ${rowNo}`}
                            >
                              <AlertTriangle className="h-4 w-4 text-amber-600" />
                            </button>
                          }
                        />
                        <TooltipContent>
                          <ul className="space-y-0.5">
                            {notes.map((n, i) => (
                              <li key={i}>{n}</li>
                            ))}
                          </ul>
                        </TooltipContent>
                      </Tooltip>
                    )}
                  </TableCell>
                </TableRow>
                {isOpen && (
                  <TableRow className="bg-muted/30 hover:bg-muted/30">
                    <TableCell />
                    <TableCell colSpan={15}>
                      <ul className="grid gap-x-6 gap-y-1 py-1 text-xs sm:grid-cols-2 lg:grid-cols-3">
                        {d.line_items.map((item, i) => (
                          <li key={i} className="flex justify-between gap-3">
                            <span>
                              {item.label ?? lineTypeLabel(item.line_type)}
                              <span className="text-muted-foreground">
                                {" "}
                                ({item.basis === "year_to_date" ? "year to date" : "this period"})
                              </span>
                            </span>
                            <span className="tabular-nums">
                              {item.amount} {d.currency}
                            </span>
                          </li>
                        ))}
                      </ul>
                    </TableCell>
                  </TableRow>
                )}
              </Fragment>
            )
          })}
        </TableBody>
      </Table>
    </div>
  )
}
