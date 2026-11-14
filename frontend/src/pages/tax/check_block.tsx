import type { ReactNode } from "react"
import type { IncomeCheckStatus } from "@/bindings/IncomeCheckStatus"
import { Button } from "@/components/ui/button"
import { CheckStatusBadge, Money } from "./tax_ui"

export interface CheckFigureRow {
  label: string
  /** The figure printed on the P60 or accounts. */
  printed: string
  /** Printed minus the summed records. */
  difference: string
}

/**
 * The reconciliation line under a source's table: printed figures against
 * the records, with the year-end adjustment action when there is a gap.
 * A check is only ever compared, never summed.
 */
export function CheckBlock({
  title, hasCheck, status, rows, currency, busy, error, onEdit, onAdd, onAdjust, addLabel,
}: {
  title: string
  hasCheck: boolean
  status?: IncomeCheckStatus
  rows: CheckFigureRow[]
  currency: string
  busy: boolean
  error: string | null
  onEdit: () => void
  onAdd: () => void
  onAdjust: () => void
  addLabel: string
}): ReactNode {
  if (!hasCheck || !status) {
    return (
      <div data-testid="check-row" className="flex flex-wrap items-center gap-3 rounded-md border border-dashed px-3 py-2 text-sm">
        <span className="font-medium">{title}</span>
        <span className="flex-1 text-muted-foreground">Not recorded for this tax year.</span>
        <Button variant="outline" size="sm" onClick={onAdd}>{addLabel}</Button>
      </div>
    )
  }
  return (
    <div data-testid="check-row" className="space-y-2 rounded-md border px-3 py-2 text-sm">
      <div className="flex flex-wrap items-center gap-3">
        <span className="font-medium">{title}</span>
        <CheckStatusBadge status={status} />
        <div className="flex-1" />
        <Button variant="ghost" size="sm" onClick={onEdit}>Edit</Button>
        {status === "gap" && (
          <Button size="sm" onClick={onAdjust} disabled={busy}>
            {busy ? "Creating..." : "Create year-end adjustment"}
          </Button>
        )}
      </div>
      <div className="flex flex-wrap gap-x-8 gap-y-1">
        {rows.map((r) => (
          <div key={r.label}>
            <span className="text-muted-foreground">{r.label}: </span>
            <Money amount={r.printed} currency={currency} />
            {Number(r.difference) !== 0 && (
              <span className="ml-1.5 text-xs text-muted-foreground">
                (records {Number(r.difference) > 0 ? "short by" : "over by"}{" "}
                <Money amount={r.difference.replace("-", "")} currency={currency} />)
              </span>
            )}
          </div>
        ))}
      </div>
      {error && <p className="text-xs text-destructive">{error}</p>}
    </div>
  )
}
