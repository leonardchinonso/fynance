import { useState } from "react"
import { api } from "@/api/client"
import type { EmploymentSourceSummary } from "@/bindings/EmploymentSourceSummary"
import type { IncomeCheck } from "@/bindings/IncomeCheck"
import type { IncomeRecord } from "@/bindings/IncomeRecord"
import type { IncomeSource } from "@/bindings/IncomeSource"
import type { LineTypeInfo } from "@/bindings/LineTypeInfo"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { ConfirmDialog } from "@/components/confirm_dialog"
import { Pencil, Plus } from "lucide-react"
import { sumMoney } from "@/lib/money"
import { CheckBlock } from "./check_block"
import { RecordsTable } from "./records_table"
import { RsuTable } from "./rsu_table"
import { visibleColumns } from "./tax_helpers"
import { Money, errorMessage } from "./tax_ui"

export function EmploymentCard({
  source, summary, records, check, catalogue, taxYear, currency,
  onEditSource, onAddRecord, onEditRecord, onEditCheck, reload,
}: {
  source: IncomeSource
  summary: EmploymentSourceSummary | undefined
  records: IncomeRecord[]
  check: IncomeCheck | undefined
  catalogue: LineTypeInfo[]
  taxYear: string
  currency: string
  onEditSource: () => void
  onAddRecord: () => void
  onEditRecord: (record: IncomeRecord) => void
  onEditCheck: (check: IncomeCheck | undefined) => void
  reload: () => void
}) {
  const [deleting, setDeleting] = useState<IncomeRecord | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [adjusting, setAdjusting] = useState(false)
  const [adjustError, setAdjustError] = useState<string | null>(null)

  async function confirmDelete() {
    if (!deleting) return
    setBusy(true)
    setError(null)
    try {
      await api.deleteIncomeRecord(deleting.id)
      setDeleting(null)
      reload()
    } catch (err) {
      setError(errorMessage(err))
    } finally {
      setBusy(false)
    }
  }

  async function adjust() {
    setAdjusting(true)
    setAdjustError(null)
    try {
      await api.createYearEndAdjustment(source.id, taxYear)
      reload()
    } catch (err) {
      setAdjustError(errorMessage(err))
    } finally {
      setAdjusting(false)
    }
  }

  const columns = visibleColumns(catalogue, "employment", records)
  const boxes = summary?.boxes
  const checkSummary = summary?.check

  return (
    <Card data-testid="employment-card">
      <CardHeader className="gap-2">
        <div className="flex flex-wrap items-center gap-2">
          <CardTitle className="text-base">{source.name}</CardTitle>
          {source.identifiers.paye_reference && (
            <span className="text-xs text-muted-foreground">PAYE {source.identifiers.paye_reference}</span>
          )}
          <div className="flex-1" />
          <Button variant="ghost" size="sm" onClick={onEditSource}>
            <Pencil className="mr-1.5 h-3.5 w-3.5" /> Edit source
          </Button>
          <Button size="sm" onClick={onAddRecord}>
            <Plus className="mr-1.5 h-4 w-4" /> Add payslip
          </Button>
        </div>
        {boxes && (
          <div className="flex flex-wrap gap-x-8 gap-y-1 text-sm">
            <div><span className="text-muted-foreground">Pay (SA102 box 1): </span><Money amount={boxes.pay} currency={currency} /></div>
            <div><span className="text-muted-foreground">Tax (box 2): </span><Money amount={boxes.tax} currency={currency} /></div>
            {Number(boxes.student_loan) + Number(boxes.postgraduate_loan) > 0 && (
              <div>
                <span className="text-muted-foreground">Student loan: </span>
                <Money amount={sumMoney([boxes.student_loan, boxes.postgraduate_loan])} currency={currency} />
              </div>
            )}
          </div>
        )}
      </CardHeader>
      <CardContent className="space-y-4">
        {records.length === 0 ? (
          <p className="text-sm text-muted-foreground">No payslips recorded for {taxYear}.</p>
        ) : (
          <RecordsTable
            records={records}
            columns={columns}
            byPayDate
            onEdit={onEditRecord}
            onDelete={(r) => { setError(null); setDeleting(r) }}
          />
        )}
        <CheckBlock
          title="P60"
          addLabel="Add P60"
          hasCheck={!!check}
          status={checkSummary?.status}
          rows={checkSummary ? [
            { label: "Pay", printed: checkSummary.pay, difference: checkSummary.pay_difference },
            { label: "Tax", printed: checkSummary.tax, difference: checkSummary.tax_difference },
          ] : []}
          currency={currency}
          busy={adjusting}
          error={adjustError}
          onAdd={() => onEditCheck(undefined)}
          onEdit={() => onEditCheck(check)}
          onAdjust={() => void adjust()}
        />
        <RsuTable rsu={summary?.rsu ?? []} records={records} currency={currency} onChanged={reload} />
      </CardContent>
      <ConfirmDialog
        open={!!deleting}
        onOpenChange={(open) => { if (!open) setDeleting(null) }}
        title="Delete record?"
        busy={busy}
        error={error}
        onConfirm={() => void confirmDelete()}
      >
        This removes the {deleting?.kind === "year_end_adjustment" ? "year-end adjustment" : "payslip"} for{" "}
        <strong>{source.name}</strong> ({deleting?.pay_date ?? deleting?.period_end}).
      </ConfirmDialog>
    </Card>
  )
}
