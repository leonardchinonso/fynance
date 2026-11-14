import type { ReactElement } from "react"
import { ApiError } from "@/api/real_service"
import type { IncomeCheckStatus } from "@/bindings/IncomeCheckStatus"
import type { TaxYearSummary } from "@/bindings/TaxYearSummary"
import type { RemoteData } from "@/lib/remote_data"
import { visitRemoteData } from "@/lib/remote_data"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { MoneyDisplay } from "@/components/currency"

export const MONEY_RE = /^\d+(\.\d+)?$/
export const SIGNED_MONEY_RE = /^-?\d+(\.\d+)?$/

export function errorMessage(err: unknown): string {
  if (err instanceof ApiError) return err.message
  return err instanceof Error ? err.message : String(err)
}

export function Money({ amount, currency }: { amount: string; currency: string }) {
  return <MoneyDisplay amount={amount} currency={currency} colorize={false} />
}

export function Dash() {
  return <span className="text-muted-foreground">-</span>
}

export function CheckStatusBadge({ status }: { status: IncomeCheckStatus }) {
  if (status === "matched") {
    return (
      <Badge variant="secondary" className="bg-emerald-500/15 text-emerald-700 dark:text-emerald-400">
        Matched
      </Badge>
    )
  }
  if (status === "adjusted") return <Badge variant="secondary">Adjusted</Badge>
  return <Badge variant="destructive">Gap</Badge>
}

export function SectionError({ error, onRetry }: { error: string; onRetry: () => void }) {
  return (
    <div className="flex flex-wrap items-center gap-2 text-sm">
      <span className="text-destructive">{error}</span>
      <Button variant="outline" size="sm" onClick={onRetry}>Retry</Button>
    </div>
  )
}

/** Loading and error handling around a card body that needs the tax-year summary. */
export function CardState({
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
    failed: (error) => <SectionError error={error} onRetry={onRetry} />,
    hasValue: children,
  })
}
