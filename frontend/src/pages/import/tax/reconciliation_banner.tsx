import { AlertTriangle, CheckCircle2 } from "lucide-react"
import { cn } from "@/lib/utils"
import type { TaxReconciliation } from "@/bindings/TaxReconciliation"

/** True when the certificate disagrees with the payslips on pay or tax. */
export function hasDifference(entry: TaxReconciliation): boolean {
  return [entry.pay_difference, entry.tax_difference].some((d) => d != null && Number(d) !== 0)
}

/**
 * One line per employer and tax year, comparing the payslips in this batch
 * with the P60: amber when they differ, green when they agree.
 */
export function ReconciliationBanner({ entries }: { entries: TaxReconciliation[] }) {
  if (entries.length === 0) return null
  return (
    <div className="space-y-1.5" data-testid="reconciliation-banner">
      {entries.map((entry, i) => {
        const differs = hasDifference(entry)
        const Icon = differs ? AlertTriangle : CheckCircle2
        return (
          <div
            key={`${entry.source_name}-${entry.tax_year}-${i}`}
            className={cn(
              "flex items-start gap-2 rounded-lg border p-2.5 text-sm",
              differs
                ? "border-amber-500/40 bg-amber-50 dark:bg-amber-950/30"
                : "border-green-600/30 bg-green-600/5"
            )}
          >
            <Icon className={cn("mt-0.5 h-4 w-4 shrink-0", differs ? "text-amber-600" : "text-green-600")} />
            <div className="min-w-0">
              <p className="text-xs font-medium">
                {entry.source_name} · {entry.tax_year}
              </p>
              <p className="text-xs text-muted-foreground">{entry.message}</p>
            </div>
          </div>
        )
      })}
    </div>
  )
}
