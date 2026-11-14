import { useState } from "react"
import { api } from "@/api/client"
import type { IncomeCheck } from "@/bindings/IncomeCheck"
import type { IncomeRecord } from "@/bindings/IncomeRecord"
import type { IncomeSource } from "@/bindings/IncomeSource"
import type { LineTypeInfo } from "@/bindings/LineTypeInfo"
import type { SelfEmploymentSummary } from "@/bindings/SelfEmploymentSummary"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { ConfirmDialog } from "@/components/confirm_dialog"
import { Pencil, Plus, RefreshCw } from "lucide-react"
import { CheckBlock } from "./check_block"
import { RecordsTable } from "./records_table"
import { visibleColumns } from "./tax_helpers"
import { Money, errorMessage } from "./tax_ui"

export function SelfEmploymentCard({
  source, summary, records, check, catalogue, taxYear, currency,
  onEditSource, onAddRecord, onEditRecord, onEditCheck, onSync, onMapCategories, reload,
}: {
  source: IncomeSource
  summary: SelfEmploymentSummary | undefined
  records: IncomeRecord[]
  check: IncomeCheck | undefined
  catalogue: LineTypeInfo[]
  taxYear: string
  currency: string
  onEditSource: () => void
  onAddRecord: () => void
  onEditRecord: (record: IncomeRecord) => void
  onEditCheck: (check: IncomeCheck | undefined) => void
  onSync: () => void
  onMapCategories: () => void
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

  const columns = visibleColumns(catalogue, "self_employment", records)
  const checkSummary = summary?.check
  const unmapped = summary?.bank_expenses.unmapped ?? []
  const info = Object.entries(summary?.info ?? {})
  const labelOf = (lt: string) => catalogue.find((c) => c.line_type === lt)?.label ?? lt

  return (
    <Card data-testid="business-card">
      <CardHeader className="gap-2">
        <div className="flex flex-wrap items-center gap-2">
          <CardTitle className="text-base">{source.name}</CardTitle>
          <Badge variant="outline" className="h-5 px-1.5 text-[10px]">Self-employment</Badge>
          <div className="flex-1" />
          <Button variant="ghost" size="sm" onClick={onEditSource}>
            <Pencil className="mr-1.5 h-3.5 w-3.5" /> Edit source
          </Button>
          {source.sync_kind === "shop_app" && (
            <Button variant="outline" size="sm" onClick={onSync}>
              <RefreshCw className="mr-1.5 h-3.5 w-3.5" /> Sync from shop
            </Button>
          )}
          <Button size="sm" onClick={onAddRecord}>
            <Plus className="mr-1.5 h-4 w-4" /> Add month
          </Button>
        </div>
        {summary && (
          <div className="flex flex-wrap gap-x-8 gap-y-1 text-sm">
            <div><span className="text-muted-foreground">Turnover: </span><Money amount={summary.turnover} currency={currency} /></div>
            <div><span className="text-muted-foreground">Expenses: </span><Money amount={summary.total_expenses} currency={currency} /></div>
            <div><span className="text-muted-foreground">Net profit: </span><Money amount={summary.net_profit} currency={currency} /></div>
          </div>
        )}
        {summary?.trading_allowance_better && (
          <p className="text-xs text-muted-foreground">
            Expenses are under the 1,000 trading allowance: claiming the allowance instead of actual expenses may leave less profit to tax.
          </p>
        )}
      </CardHeader>
      <CardContent className="space-y-4">
        {records.length === 0 ? (
          <p className="text-sm text-muted-foreground">No sales recorded for {taxYear}.</p>
        ) : (
          <RecordsTable
            records={records}
            columns={columns}
            byPayDate={false}
            onEdit={onEditRecord}
            onDelete={(r) => { setError(null); setDeleting(r) }}
          />
        )}

        {summary && (
          <div className="space-y-1.5 text-sm">
            <div className="flex flex-wrap items-center gap-3">
              <span className="font-medium">Business-account expenses</span>
              <span className="text-muted-foreground">
                <Money amount={summary.bank_expenses.mapped_total} currency={currency} /> mapped
              </span>
              <div className="flex-1" />
              <Button variant="outline" size="sm" onClick={onMapCategories}>Map expense categories</Button>
            </div>
            {unmapped.length > 0 && (
              <div className="flex flex-wrap items-center gap-1.5">
                <span className="text-xs text-muted-foreground">Unmapped, not counted:</span>
                {unmapped.map((u) => (
                  <Badge key={u.category_id} variant="destructive" className="gap-1">
                    {u.name} <Money amount={u.total} currency={currency} />
                  </Badge>
                ))}
              </div>
            )}
          </div>
        )}

        <CheckBlock
          title="Annual accounts"
          addLabel="Add annual accounts"
          hasCheck={!!check}
          status={checkSummary?.status}
          rows={checkSummary ? [
            { label: "Turnover", printed: checkSummary.turnover, difference: checkSummary.turnover_difference },
            { label: "Expenses", printed: checkSummary.expenses, difference: checkSummary.expenses_difference },
          ] : []}
          currency={currency}
          busy={adjusting}
          error={adjustError}
          onAdd={() => onEditCheck(undefined)}
          onEdit={() => onEditCheck(check)}
          onAdjust={() => void adjust()}
        />

        {info.length > 0 && (
          <details className="text-sm">
            <summary className="cursor-pointer text-muted-foreground">Shop-app costings (information only, not summed)</summary>
            <div className="mt-2 flex flex-wrap gap-x-8 gap-y-1">
              {info.map(([lt, amount]) => (
                <div key={lt}>
                  <span className="text-muted-foreground">{labelOf(lt)}: </span>
                  <Money amount={amount ?? "0.00"} currency={currency} />
                </div>
              ))}
            </div>
          </details>
        )}
      </CardContent>
      <ConfirmDialog
        open={!!deleting}
        onOpenChange={(open) => { if (!open) setDeleting(null) }}
        title="Delete record?"
        busy={busy}
        error={error}
        onConfirm={() => void confirmDelete()}
      >
        This removes the {deleting?.kind === "year_end_adjustment" ? "year-end adjustment" : "month"} for{" "}
        <strong>{source.name}</strong> ({deleting?.period_start} to {deleting?.period_end}).
      </ConfirmDialog>
    </Card>
  )
}
