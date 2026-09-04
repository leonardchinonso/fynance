import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Separator } from "@/components/ui/separator"
import { formatCurrency } from "@/lib/utils"
import { useRedactedFlag } from "@/hooks/use_redacted_flag"
import type { CgtSummary } from "@/bindings/CgtSummary"
import type { TaxComputation } from "@/bindings/TaxComputation"

interface CgtSummaryCardProps {
  summary: CgtSummary
  disposalCount: number
  /**
   * The server's tax computation, when the report was run for a whole tax year.
   * Absent means it was not asked for — never that no tax is due.
   */
  tax?: TaxComputation | null
}

export function CgtSummaryCard({ summary, disposalCount, tax }: CgtSummaryCardProps) {
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
        {tax && (
          <>
            <Separator />
            {Number.parseFloat(tax.current_year_losses_applied) > 0 && (
              <Row
                label="Losses in the year, set against gains"
                value={`-${formatCurrency(tax.current_year_losses_applied, cur)}`}
              />
            )}
            {Number.parseFloat(tax.brought_forward_losses_applied) > 0 && (
              <>
                <Row
                  label="Brought-forward losses used"
                  value={`-${formatCurrency(tax.brought_forward_losses_applied, cur)}`}
                />
                {/*
                  This caveat has to live where the figure is actually SHOWN.
                  It previously appeared only inside the pre-flight screen,
                  which opens solely on `missing_exchange_rates` — the same
                  condition that makes the derive endpoint refuse — so it could
                  never be read. A brought-forward figure that carries no
                  qualification overstates relief: losses carry forward only if
                  claimed within four years of the end of the year they arose,
                  and only the part left after that year's own gains carries at
                  all. Neither is something this app can verify.
                */}
                <p className="text-xs text-muted-foreground">
                  Losses carry forward only if you claimed them within four years of the end
                  of the year they arose, and only the part left after that year&rsquo;s own
                  gains carries at all. Check this against your filed returns before using it.
                </p>
              </>
            )}
            <Row
              label={`Annual Exempt Amount (${tax.tax_year})`}
              value={`-${formatCurrency(tax.aea_applied, cur)}`}
            />
            <Row
              label="Taxable gain after losses and allowance"
              value={formatCurrency(tax.taxable_gain, cur)}
            />
            <Separator />
            <Row
              label="Capital Gains Tax due"
              value={formatCurrency(tax.tax_due, cur)}
              emphasis
            />
          </>
        )}
        <p className="text-xs text-muted-foreground pt-2">
          {tax
            ? `Computed for tax year ${tax.tax_year} from the stored tax configuration and your recorded figures. Gains are charged at the rate in force on the date of each disposal, and losses and the allowance are set against the most heavily taxed gains first. Estimate only — disposals made outside this app are not included.`
            : "Figures are pre-relief: choose a whole tax year to apply the Annual Exempt Amount, brought-forward losses and the rates in force."}
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
