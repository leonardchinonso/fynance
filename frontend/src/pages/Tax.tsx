import { useMemo, useState, type ReactElement, type ReactNode } from "react"
import { api } from "@/api/client"
import { ApiError } from "@/api/real_service"
import { ukTaxYearForDate, ukTaxYearToDates } from "@/api/cgt_filter_params"
import type {
  EmploymentPay,
  EmploymentPayPayload,
  TaxYearSummary,
} from "@/types/tax_v1"
import { useEmploymentPay, useTaxSummary } from "@/hooks/data"
import type { RemoteData } from "@/lib/remote_data"
import { visitRemoteData } from "@/lib/remote_data"
import { sumMoney } from "@/lib/money"
import { cn } from "@/lib/utils"
import { usePreferredCurrency } from "@/context/preferred_currency_context"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import { Textarea } from "@/components/ui/textarea"
import { Switch } from "@/components/ui/switch"
import {
  Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle,
} from "@/components/ui/dialog"
import {
  Select, SelectContent, SelectItem, SelectTrigger,
} from "@/components/ui/select"
import {
  Table, TableBody, TableCell, TableHead, TableHeader, TableRow,
} from "@/components/ui/table"
import { EmptyState } from "@/components/empty_state"
import { ConfirmDialog } from "@/components/confirm_dialog"
import { ReloadingOverlay } from "@/components/reloading_overlay"
import { TableSkeleton } from "@/components/skeletons"
import { MoneyDisplay } from "@/components/currency"
import { Check, Pencil, Plus, Trash2 } from "lucide-react"

const MONEY_RE = /^\d+(\.\d+)?$/
const TAX_YEAR_RE = /^\d{4}-\d{2}$/

/** Rows for the selected year, in the server's order (period_end asc, NULLs last). */
function rowsForYear(rows: EmploymentPay[], taxYear: string): EmploymentPay[] {
  return rows.filter((r) => r.tax_year === taxYear)
}

/** Year window subtitle; empty for a malformed year coming from the server. */
function yearWindow(taxYear: string): string {
  try {
    const [start, end] = ukTaxYearToDates(taxYear)
    return `${start} to ${end}`
  } catch {
    return ""
  }
}

export function TaxPage() {
  const currentTaxYear = useMemo(() => ukTaxYearForDate(new Date()), [])
  const [taxYear, setTaxYear] = useState(currentTaxYear)

  const [rowsData, reloadRows] = useEmploymentPay()
  const [summaryData, reloadSummary] = useTaxSummary(taxYear)

  const yearOptions = useMemo(() => {
    const years = new Set<string>([currentTaxYear])
    if (rowsData.status === "succeeded" || rowsData.status === "reloading") {
      for (const r of rowsData.value) years.add(r.tax_year)
    }
    return [...years].sort((a, b) => b.localeCompare(a))
  }, [rowsData, currentTaxYear])

  const [editing, setEditing] = useState<EmploymentPay | null>(null)
  const [adding, setAdding] = useState(false)

  function reloadAll() {
    reloadRows()
    reloadSummary()
  }

  const windowLabel = yearWindow(taxYear)

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center gap-3">
        <h1 className="text-lg font-semibold">Tax</h1>
        <Select value={taxYear} onValueChange={(v) => v && setTaxYear(v)}>
          <SelectTrigger className="w-[150px]">
            <span>{taxYear}</span>
          </SelectTrigger>
          <SelectContent>
            {yearOptions.map((y) => (
              <SelectItem key={y} value={y}>{y === currentTaxYear ? `${y} (current)` : y}</SelectItem>
            ))}
          </SelectContent>
        </Select>
        {windowLabel && <span className="text-sm text-muted-foreground">{windowLabel}</span>}
        <div className="flex-1" />
        <Button size="sm" onClick={() => setAdding(true)}>
          <Plus className="mr-1.5 h-4 w-4" />
          Add employment pay
        </Button>
      </div>

      <ReadinessChecklist data={summaryData} taxYear={taxYear} onRetry={reloadSummary} />

      <div className="grid gap-4 lg:grid-cols-2">
        <InterestCard data={summaryData} taxYear={taxYear} onRetry={reloadSummary} />
        <GiftAidCard data={summaryData} taxYear={taxYear} onRetry={reloadSummary} />
      </div>

      <EmploymentSection
        data={rowsData}
        taxYear={taxYear}
        reload={reloadAll}
        onAdd={() => setAdding(true)}
        onEdit={setEditing}
      />

      {(adding || editing) && (
        <EmploymentPayDialog
          row={editing}
          defaultTaxYear={taxYear}
          onClose={() => { setAdding(false); setEditing(null) }}
          onSaved={() => { setAdding(false); setEditing(null); reloadAll() }}
        />
      )}
    </div>
  )
}

// ── Readiness checklist ───────────────────────────────────────────────────────

function StatusChip({ ok }: { ok: boolean }) {
  return ok ? (
    <Badge variant="secondary" className="gap-1 bg-emerald-500/15 text-emerald-700 dark:text-emerald-400">
      <Check className="h-3 w-3" /> OK
    </Badge>
  ) : (
    <Badge variant="destructive">Needs attention</Badge>
  )
}

function ReadinessChecklist({
  data, taxYear, onRetry,
}: {
  data: RemoteData<TaxYearSummary>
  taxYear: string
  onRetry: () => void
}) {
  const preferred = usePreferredCurrency()
  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">Readiness for {taxYear}</CardTitle>
      </CardHeader>
      <CardContent>
        {visitRemoteData(data, {
          notLoaded: () => (
            <div className="space-y-2" aria-busy="true">
              {[0, 1, 2].map((i) => (
                <div key={i} className="h-5 w-full rounded bg-muted/60 animate-pulse" />
              ))}
            </div>
          ),
          failed: (error) => (
            <div className="flex flex-wrap items-center gap-2 text-sm">
              <span className="text-destructive">{error}</span>
              <Button variant="outline" size="sm" onClick={onRetry}>Retry</Button>
            </div>
          ),
          hasValue: (summary) => {
            const emp = summary.employment
            const interest = summary.interest
            const money = (amount: string) => (
              <MoneyDisplay amount={amount} currency={preferred} colorize={false} />
            )
            const rows: { label: string; detail: ReactNode; ok: boolean }[] = [
              {
                label: "Employment",
                detail: emp.payslip_count === 0 && !emp.has_p60
                  ? "No PAYE records for this tax year yet"
                  : `${emp.payslip_count} payslip${emp.payslip_count === 1 ? "" : "s"}, P60 ${emp.has_p60 ? "recorded" : "missing"}`,
                ok: emp.payslip_count > 0 || emp.has_p60,
              },
              {
                label: "Interest",
                detail: interest.by_account.length === 0 ? (
                  "No interest totals for this tax year"
                ) : (
                  <>
                    {interest.by_account.length} account{interest.by_account.length === 1 ? "" : "s"}:{" "}
                    {money(interest.taxable_total)} taxable, {money(interest.non_taxable_total)} tax-free
                  </>
                ),
                ok: interest.by_account.length > 0,
              },
              {
                label: "Gift Aid",
                detail: Number(summary.donations.gift_aid_total) === 0 ? (
                  "Nil for this tax year"
                ) : (
                  <>
                    {money(summary.donations.gift_aid_total)} Gift Aid,{" "}
                    {money(summary.donations.non_gift_aid_total)} not Gift Aid
                  </>
                ),
                ok: true,
              },
            ]
            return (
              <div className="space-y-2">
                {rows.map((row) => (
                  <div key={row.label} className="flex items-center gap-3 text-sm">
                    <span className="w-28 shrink-0 font-medium">{row.label}</span>
                    <span className="flex-1 min-w-0 text-muted-foreground">{row.detail}</span>
                    <StatusChip ok={row.ok} />
                  </div>
                ))}
                {emp.has_unknown && (
                  <p className="text-xs text-muted-foreground">
                    Some records have unknown amounts; unknown legs are summed as 0.
                  </p>
                )}
              </div>
            )
          },
        })}
      </CardContent>
    </Card>
  )
}

// ── Interest / Gift Aid cards ─────────────────────────────────────────────────

function CardState({
  data, onRetry, children,
}: {
  data: RemoteData<TaxYearSummary>
  onRetry: () => void
  children: (summary: TaxYearSummary) => ReactElement
}) {
  return visitRemoteData(data, {
    notLoaded: () => (
      <div className="space-y-2" aria-busy="true">
        <div className="h-5 w-2/3 rounded bg-muted/60 animate-pulse" />
        <div className="h-5 w-1/2 rounded bg-muted/60 animate-pulse" />
      </div>
    ),
    failed: (error) => (
      <div className="flex flex-wrap items-center gap-2 text-sm">
        <span className="text-destructive">{error}</span>
        <Button variant="outline" size="sm" onClick={onRetry}>Retry</Button>
      </div>
    ),
    hasValue: children,
  })
}

function InterestCard({
  data, taxYear, onRetry,
}: {
  data: RemoteData<TaxYearSummary>
  taxYear: string
  onRetry: () => void
}) {
  const preferred = usePreferredCurrency()
  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">Interest in {taxYear}</CardTitle>
      </CardHeader>
      <CardContent>
        <CardState data={data} onRetry={onRetry}>
          {(summary) => {
            const interest = summary.interest
            if (interest.by_account.length === 0) {
              return (
                <p className="text-sm text-muted-foreground">
                  No interest totals for this tax year. Interest is captured from
                  transactions categorized as taxable or tax-free interest.
                </p>
              )
            }
            return (
              <div className="relative">
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>Account</TableHead>
                      <TableHead className="text-right">Taxable</TableHead>
                      <TableHead className="text-right">Tax-free</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {interest.by_account.map((a) => (
                      <TableRow key={a.account_id}>
                        <TableCell className="max-w-[180px] truncate">{a.account_name}</TableCell>
                        <TableCell className="text-right">
                          <MoneyDisplay amount={a.taxable} currency={preferred} colorize={false} />
                        </TableCell>
                        <TableCell className="text-right">
                          <MoneyDisplay amount={a.non_taxable} currency={preferred} colorize={false} />
                        </TableCell>
                      </TableRow>
                    ))}
                    <TableRow className="border-t-2 font-medium">
                      <TableCell>Total</TableCell>
                      <TableCell className="text-right">
                        <MoneyDisplay amount={interest.taxable_total} currency={preferred} colorize={false} />
                      </TableCell>
                      <TableCell className="text-right">
                        <MoneyDisplay amount={interest.non_taxable_total} currency={preferred} colorize={false} />
                      </TableCell>
                    </TableRow>
                  </TableBody>
                </Table>
              </div>
            )
          }}
        </CardState>
      </CardContent>
    </Card>
  )
}

function GiftAidCard({
  data, taxYear, onRetry,
}: {
  data: RemoteData<TaxYearSummary>
  taxYear: string
  onRetry: () => void
}) {
  const preferred = usePreferredCurrency()
  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">Donations in {taxYear}</CardTitle>
      </CardHeader>
      <CardContent>
        <CardState data={data} onRetry={onRetry}>
          {(summary) => (
            <div className="space-y-3">
              <div className="flex gap-8">
                <div>
                  <p className="text-xs text-muted-foreground">Gift Aid</p>
                  <MoneyDisplay
                    amount={summary.donations.gift_aid_total}
                    currency={preferred}
                    colorize={false}
                    className="text-sm font-medium"
                  />
                </div>
                <div>
                  <p className="text-xs text-muted-foreground">Non-Gift-Aid</p>
                  <MoneyDisplay
                    amount={summary.donations.non_gift_aid_total}
                    currency={preferred}
                    colorize={false}
                    className="text-sm font-medium"
                  />
                </div>
              </div>
              <p className="text-xs text-muted-foreground">
                Gift Aid is the donation_taxable category type; non-reclaimable donations are donation_non_taxable.
              </p>
            </div>
          )}
        </CardState>
      </CardContent>
    </Card>
  )
}

// ── Employment pay table ──────────────────────────────────────────────────────

interface CurrencyTotals {
  currency: string
  gross: string
  tax: string
  ni: string | null
  studentLoan: string | null
  net: string | null
}

function totalsByCurrency(rows: EmploymentPay[]): CurrencyTotals[] {
  const groups = new Map<string, EmploymentPay[]>()
  for (const r of rows) {
    const list = groups.get(r.currency) ?? []
    list.push(r)
    groups.set(r.currency, list)
  }
  const sumOrNull = (list: EmploymentPay[], pick: (r: EmploymentPay) => string | null): string | null =>
    list.some((r) => pick(r) != null) ? sumMoney(list.map(pick)) : null
  return [...groups.entries()].map(([currency, list]) => ({
    currency,
    gross: sumMoney(list.map((r) => r.gross_pay)),
    tax: sumMoney(list.map((r) => r.tax_deducted)),
    ni: sumOrNull(list, (r) => r.employees_ni),
    studentLoan: sumOrNull(list, (r) => r.student_loan),
    net: sumOrNull(list, (r) => r.net_pay),
  }))
}

function EmploymentSection({
  data, taxYear, reload, onAdd, onEdit,
}: {
  data: RemoteData<EmploymentPay[]>
  taxYear: string
  reload: () => void
  onAdd: () => void
  onEdit: (row: EmploymentPay) => void
}) {
  const [deleting, setDeleting] = useState<EmploymentPay | null>(null)
  const [deleteBusy, setDeleteBusy] = useState(false)
  const [deleteError, setDeleteError] = useState<string | null>(null)

  async function handleDeleteConfirm() {
    if (!deleting) return
    setDeleteBusy(true)
    setDeleteError(null)
    try {
      await api.deleteEmploymentPay(deleting.id)
      setDeleting(null)
      reload()
    } catch (err) {
      setDeleteError(err instanceof Error ? err.message : String(err))
    } finally {
      setDeleteBusy(false)
    }
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">Employment pay in {taxYear}</CardTitle>
      </CardHeader>
      <CardContent>
        {visitRemoteData(data, {
          notLoaded: () => <TableSkeleton rows={4} cols={7} />,
          failed: (error) => (
            <div className="flex flex-wrap items-center gap-2 text-sm">
              <span className="text-destructive">{error}</span>
              <Button variant="outline" size="sm" onClick={reload}>Retry</Button>
            </div>
          ),
          hasValue: (allRows) => {
            const rows = rowsForYear(allRows, taxYear)
            if (rows.length === 0) {
              const anyRowsExist = allRows.length > 0
              return (
                <EmptyState
                  title={anyRowsExist
                    ? `No employment pay records for ${taxYear}`
                    : "No employment pay records yet"}
                  message={anyRowsExist
                    ? "Payslips and P60s recorded for other tax years are listed when you switch years. Add one for this year with the button above."
                    : "Record payslips and P60s here, or push them through the employment-pay API; they never appear as bank movements."}
                  action={{ label: "Add employment pay", onClick: onAdd }}
                />
              )
            }
            const totals = totalsByCurrency(rows)
            return (
              <div className="relative">
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>Period</TableHead>
                      <TableHead>Employer</TableHead>
                      <TableHead className="text-right">Gross pay</TableHead>
                      <TableHead className="text-right">Tax deducted</TableHead>
                      <TableHead className="text-right">NI</TableHead>
                      <TableHead className="text-right">Student loan</TableHead>
                      <TableHead className="text-right">Net pay</TableHead>
                      <TableHead />
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {rows.map((r) => (
                      <TableRow key={r.id}>
                        <TableCell className="whitespace-nowrap">
                          {r.is_p60 ? (
                            <span className="inline-flex items-center gap-1.5">
                              Annual
                              <Badge variant="secondary" className="h-5 px-1.5 text-[10px]">P60</Badge>
                            </span>
                          ) : r.period_start || r.period_end ? (
                            `${r.period_start ?? "?"} – ${r.period_end ?? "?"}`
                          ) : (
                            "—"
                          )}
                        </TableCell>
                        <TableCell className="max-w-[180px] truncate">
                          {r.employer}
                          {r.transaction_id && (
                            <span className="ml-1.5 text-xs text-muted-foreground" title="Linked to a bank transaction">
                              linked
                            </span>
                          )}
                        </TableCell>
                        <TableCell className="text-right">
                          <MoneyDisplay amount={r.gross_pay} currency={r.currency} colorize={false} />
                        </TableCell>
                        <TableCell className="text-right">
                          <MoneyDisplay amount={r.tax_deducted} currency={r.currency} colorize={false} />
                        </TableCell>
                        <TableCell className="text-right">
                          {r.employees_ni != null
                            ? <MoneyDisplay amount={r.employees_ni} currency={r.currency} colorize={false} />
                            : <span className="text-muted-foreground">—</span>}
                        </TableCell>
                        <TableCell className="text-right">
                          {r.student_loan != null
                            ? <MoneyDisplay amount={r.student_loan} currency={r.currency} colorize={false} />
                            : <span className="text-muted-foreground">—</span>}
                        </TableCell>
                        <TableCell className="text-right">
                          {r.net_pay != null
                            ? <MoneyDisplay amount={r.net_pay} currency={r.currency} colorize={false} />
                            : <span className="text-muted-foreground">—</span>}
                        </TableCell>
                        <TableCell className="whitespace-nowrap text-right">
                          <Button variant="ghost" size="icon" className="h-7 w-7" onClick={() => onEdit(r)} title="Edit">
                            <Pencil className="h-3.5 w-3.5" />
                          </Button>
                          <Button
                            variant="ghost"
                            size="icon"
                            className="h-7 w-7"
                            onClick={() => { setDeleteError(null); setDeleting(r) }}
                            title="Delete"
                          >
                            <Trash2 className="h-3.5 w-3.5 text-muted-foreground" />
                          </Button>
                        </TableCell>
                      </TableRow>
                    ))}
                    {totals.map((t) => (
                      <TableRow key={`total-${t.currency}`} className="border-t-2 font-medium">
                        <TableCell>{totals.length > 1 ? `Total (${t.currency})` : "Total"}</TableCell>
                        <TableCell />
                        <TableCell className="text-right">
                          <MoneyDisplay amount={t.gross} currency={t.currency} colorize={false} />
                        </TableCell>
                        <TableCell className="text-right">
                          <MoneyDisplay amount={t.tax} currency={t.currency} colorize={false} />
                        </TableCell>
                        <TableCell className="text-right">
                          {t.ni != null
                            ? <MoneyDisplay amount={t.ni} currency={t.currency} colorize={false} />
                            : <span className="text-muted-foreground">—</span>}
                        </TableCell>
                        <TableCell className="text-right">
                          {t.studentLoan != null
                            ? <MoneyDisplay amount={t.studentLoan} currency={t.currency} colorize={false} />
                            : <span className="text-muted-foreground">—</span>}
                        </TableCell>
                        <TableCell className="text-right">
                          {t.net != null
                            ? <MoneyDisplay amount={t.net} currency={t.currency} colorize={false} />
                            : <span className="text-muted-foreground">—</span>}
                        </TableCell>
                        <TableCell />
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
                <ReloadingOverlay active={data.status === "reloading"} />
              </div>
            )
          },
        })}
      </CardContent>

      <ConfirmDialog
        open={!!deleting}
        onOpenChange={(open) => { if (!open) setDeleting(null) }}
        title="Delete employment pay record?"
        busy={deleteBusy}
        error={deleteError}
        onConfirm={handleDeleteConfirm}
      >
        This removes the {deleting?.is_p60 ? "P60 summary" : "payslip"} for{" "}
        <strong>{deleting?.employer}</strong>
        {deleting?.period_start ? <> covering {deleting.period_start} to {deleting.period_end}</> : null}.
      </ConfirmDialog>
    </Card>
  )
}

// ── Add / edit dialog ─────────────────────────────────────────────────────────

interface EntryFormState {
  taxYear: string
  employer: string
  periodStart: string
  periodEnd: string
  isP60: boolean
  grossPay: string
  taxDeducted: string
  employeesNi: string
  studentLoan: string
  netPay: string
  currency: string
  transactionId: string
  notes: string
}

function formFromRow(row: EmploymentPay | null, defaultTaxYear: string): EntryFormState {
  return {
    taxYear: row?.tax_year ?? defaultTaxYear,
    employer: row?.employer ?? "",
    periodStart: row?.period_start ?? "",
    periodEnd: row?.period_end ?? "",
    isP60: row?.is_p60 ?? false,
    grossPay: row?.gross_pay ?? "",
    taxDeducted: row?.tax_deducted ?? "",
    employeesNi: row?.employees_ni ?? "",
    studentLoan: row?.student_loan ?? "",
    netPay: row?.net_pay ?? "",
    currency: row?.currency ?? "GBP",
    transactionId: row?.transaction_id ?? "",
    notes: row?.notes ?? "",
  }
}

/** Blank means absent; a blanking edit sends null so the server clears the column. */
function optionalMoney(value: string): string | null {
  return value.trim() === "" ? null : value.trim()
}

function EmploymentPayDialog({
  row, defaultTaxYear, onClose, onSaved,
}: {
  row: EmploymentPay | null
  defaultTaxYear: string
  onClose: () => void
  onSaved: () => void
}) {
  const isEdit = row !== null
  const [form, setForm] = useState<EntryFormState>(() => formFromRow(row, defaultTaxYear))
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  function update<K extends keyof EntryFormState>(key: K, value: EntryFormState[K]) {
    setForm((f) => ({ ...f, [key]: value }))
  }

  function moneyValid(value: string, required: boolean): boolean {
    const v = value.trim()
    if (v === "") return !required
    return MONEY_RE.test(v)
  }

  const valid =
    TAX_YEAR_RE.test(form.taxYear) &&
    form.employer.trim() !== "" &&
    moneyValid(form.grossPay, true) &&
    moneyValid(form.taxDeducted, true) &&
    moneyValid(form.employeesNi, false) &&
    moneyValid(form.studentLoan, false) &&
    moneyValid(form.netPay, false) &&
    form.currency.trim() !== ""

  async function handleSave() {
    if (!valid) {
      setError("Check the highlighted fields: employer and tax year are required, money fields must be non-negative decimals.")
      return
    }
    setSaving(true)
    setError(null)
    const payload: EmploymentPayPayload = {
      tax_year: form.taxYear.trim(),
      employer: form.employer.trim(),
      period_start: form.periodStart.trim() === "" ? null : form.periodStart.trim(),
      period_end: form.periodEnd.trim() === "" ? null : form.periodEnd.trim(),
      is_p60: form.isP60,
      gross_pay: form.grossPay.trim(),
      tax_deducted: form.taxDeducted.trim(),
      employees_ni: optionalMoney(form.employeesNi),
      student_loan: optionalMoney(form.studentLoan),
      net_pay: optionalMoney(form.netPay),
      currency: form.currency.trim().toUpperCase(),
      transaction_id: form.transactionId.trim() === "" ? null : form.transactionId.trim(),
      document_ids: row?.document_ids ?? [],
      notes: form.notes.trim() === "" ? null : form.notes.trim(),
    }
    try {
      if (isEdit && row) {
        await api.updateEmploymentPay(row.id, payload)
      } else {
        await api.createEmploymentPay(payload)
      }
      onSaved()
    } catch (err) {
      setError(err instanceof ApiError ? err.message : err instanceof Error ? err.message : String(err))
    } finally {
      setSaving(false)
    }
  }

  const fieldClass = (key: "grossPay" | "taxDeducted" | "employeesNi" | "studentLoan" | "netPay", required = false) =>
    cn(!moneyValid(form[key], required) && "border-destructive")

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="sm:max-w-lg max-h-[90vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{isEdit ? "Edit employment pay record" : "Add employment pay record"}</DialogTitle>
        </DialogHeader>
        <div className="space-y-3 pt-2">
          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="text-sm font-medium">Tax year</label>
              <Input
                placeholder="2025-26"
                value={form.taxYear}
                onChange={(e) => update("taxYear", e.target.value)}
                className={cn(!TAX_YEAR_RE.test(form.taxYear) && "border-destructive")}
              />
            </div>
            <div>
              <label className="text-sm font-medium">Employer</label>
              <Input
                placeholder="e.g. Employer A"
                value={form.employer}
                onChange={(e) => update("employer", e.target.value)}
                autoFocus={!isEdit}
              />
            </div>
          </div>

          <label className="flex items-center justify-between rounded-md border px-3 py-2">
            <span className="text-sm font-medium">P60 / annual summary row</span>
            <Switch size="sm" checked={form.isP60} onCheckedChange={(v) => update("isP60", v)} />
          </label>

          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="text-sm font-medium">Period start</label>
              <Input type="date" value={form.periodStart} onChange={(e) => update("periodStart", e.target.value)} />
            </div>
            <div>
              <label className="text-sm font-medium">Period end</label>
              <Input type="date" value={form.periodEnd} onChange={(e) => update("periodEnd", e.target.value)} />
            </div>
          </div>

          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="text-sm font-medium">Gross pay</label>
              <Input
                inputMode="decimal"
                placeholder="1000.00"
                value={form.grossPay}
                onChange={(e) => update("grossPay", e.target.value)}
                className={fieldClass("grossPay", true)}
              />
            </div>
            <div>
              <label className="text-sm font-medium">Tax deducted</label>
              <Input
                inputMode="decimal"
                placeholder="100.00"
                value={form.taxDeducted}
                onChange={(e) => update("taxDeducted", e.target.value)}
                className={fieldClass("taxDeducted", true)}
              />
            </div>
          </div>

          <div className="grid grid-cols-3 gap-3">
            <div>
              <label className="text-sm font-medium">Employee NI</label>
              <Input
                inputMode="decimal"
                placeholder="optional"
                value={form.employeesNi}
                onChange={(e) => update("employeesNi", e.target.value)}
                className={fieldClass("employeesNi")}
              />
            </div>
            <div>
              <label className="text-sm font-medium">Student loan</label>
              <Input
                inputMode="decimal"
                placeholder="optional"
                value={form.studentLoan}
                onChange={(e) => update("studentLoan", e.target.value)}
                className={fieldClass("studentLoan")}
              />
            </div>
            <div>
              <label className="text-sm font-medium">Net pay</label>
              <Input
                inputMode="decimal"
                placeholder="optional"
                value={form.netPay}
                onChange={(e) => update("netPay", e.target.value)}
                className={fieldClass("netPay")}
              />
            </div>
          </div>

          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="text-sm font-medium">Currency</label>
              <Input
                placeholder="GBP"
                value={form.currency}
                onChange={(e) => update("currency", e.target.value.toUpperCase())}
              />
            </div>
            <div>
              <label className="text-sm font-medium">Linked transaction id</label>
              <Input
                placeholder="optional"
                value={form.transactionId}
                onChange={(e) => update("transactionId", e.target.value)}
              />
            </div>
          </div>

          <div>
            <label className="text-sm font-medium">Notes (optional)</label>
            <Textarea placeholder="Any additional notes" value={form.notes} onChange={(e) => update("notes", e.target.value)} />
          </div>

          {error && <p className="text-xs text-destructive">{error}</p>}
        </div>
        <DialogFooter>
          <Button variant="outline" size="sm" onClick={onClose}>Cancel</Button>
          <Button size="sm" onClick={handleSave} disabled={!valid || saving}>
            {saving ? "Saving..." : isEdit ? "Save" : "Create"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
