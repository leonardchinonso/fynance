import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Separator } from "@/components/ui/separator"
import { formatCurrency } from "@/lib/utils"
import { useRedactedFlag } from "@/hooks/use_redacted_flag"
import type { CgtSummary } from "@/bindings/CgtSummary"

interface CgtSummaryCardProps {
  summary: CgtSummary
  disposalCount: number
}

export function CgtSummaryCard({ summary, disposalCount }: CgtSummaryCardProps) {
  useRedactedFlag()
  const cur = summary.base_currency
  const net = Number.parseFloat(summary.net_gain_loss)
  return (
    <Card>
      <CardHeader>
        <CardTitle>Summary</CardTitle>
      </CardHeader>
      <CardContent className="space-y-3">
        <Row label="Number of disposals" value={String(disposalCount)} />
        <Row label="Disposal proceeds" value={formatCurrency(summary.total_proceeds, cur)} />
        <Row
          label="Allowable costs (including purchase price)"
          value={formatCurrency(summary.total_allowable_costs, cur)}
        />
        <Row
          label="Gains in the year, before losses"
          value={formatCurrency(summary.total_gains, cur)}
        />
        <Row label="Losses in the year" value={formatCurrency(summary.total_losses, cur)} />
        <Separator />
        <Row
          label="Net gain/loss in the year"
          value={formatCurrency(summary.net_gain_loss, cur)}
          emphasis
          tone={net >= 0 ? "gain" : "loss"}
        />
        <p className="text-xs text-muted-foreground pt-2">
          Figures are pre-relief. Annual Exempt Amount, brought-forward losses, and
          tax-year rate adjustments are not applied — see plan 23 for the post-V0
          roadmap towards filing-grade output.
        </p>
      </CardContent>
    </Card>
  )
}

function Row({
  label,
  value,
  emphasis = false,
  tone,
}: {
  label: string
  value: string
  emphasis?: boolean
  tone?: "gain" | "loss"
}) {
  return (
    <div className="flex items-baseline justify-between gap-4">
      <span className={emphasis ? "text-sm font-semibold" : "text-sm text-muted-foreground"}>
        {label}
      </span>
      <span
        className={
          emphasis
            ? tone === "gain"
              ? "tabular-nums text-base font-semibold text-emerald-600 dark:text-emerald-400"
              : tone === "loss"
                ? "tabular-nums text-base font-semibold text-red-600 dark:text-red-400"
                : "tabular-nums text-base font-semibold"
            : "tabular-nums text-sm"
        }
      >
        {value}
      </span>
    </div>
  )
}
