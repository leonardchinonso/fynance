import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Separator } from "@/components/ui/separator"
import { formatCurrency } from "@/lib/utils"
import { useRedactedFlag } from "@/hooks/use_redacted_flag"
import type { CgtSummary } from "@/bindings/CgtSummary"
import type { TaxBandResult } from "@/bindings/TaxBandResult"
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
  // Only bands that actually bear tax are worth a row; a band fully covered by
  // losses or the allowance would print as a £0.00 line that reads like an error.
  const taxableBands = (tax?.bands ?? []).filter((b) => Number.parseFloat(b.taxable) > 0)
  // Whether the year splits across more than one *period* (as 2024-25 does, at
  // 30 October 2024). Decides whether band labels need to name their date range:
  // with a single period, "Gains from 6 Apr 2024 @ 24%" is noise.
  const multipleBandPeriods = new Set(taxableBands.map((b) => b.valid_from)).size > 1
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
            {/*
              The rate bands are the working behind the total below. Rendered
              with the plain (non-emphasis) Row so they read as the derivation
              and "Capital Gains Tax due" stays the bottom line — the same
              hierarchy the PDF uses. The label wording is deliberately mirrored
              from cgt_pdf_document.tsx so the screen and the filed document
              cannot drift; see bandLabel below.
            */}
            {taxableBands.map((b) => (
              <Row
                key={`${b.valid_from}-${b.rate_kind}`}
                label={bandLabel(b, multipleBandPeriods)}
                value={formatCurrency(b.tax, cur)}
              />
            ))}
            {Number.parseFloat(tax.total_gains) > 0 && taxableBands.length === 0 && (
              <p className="text-xs text-muted-foreground">
                No Capital Gains Tax due for {tax.tax_year}: the gains are covered by losses
                and the Annual Exempt Amount.
              </p>
            )}
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

/*
 * fmtBandDate and bandLabel below are intentionally DUPLICATED from
 * cgt_pdf_document.tsx rather than shared. The PDF is built from
 * @react-pdf/renderer primitives and this card from DOM + Tailwind; extracting
 * a shared helper would drag both modules into one another's scope for the sake
 * of ten lines. The wording is mirrored verbatim so the screen and the filed
 * document cannot disagree about which rate produced which figure — if you edit
 * one, edit the other.
 */

function fmtBandDate(iso: string): string {
  const months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]
  const [y, m, d] = iso.split("-")
  return `${Number(d)} ${months[Number(m) - 1]} ${y}`
}

/**
 * Label for one rate band row.
 *
 * The rate comes from the server as a fraction, so it is rendered as a
 * percentage here and nowhere else. `multiplePeriods` is why the date range is
 * conditional: naming it is essential when a year splits (2024-25 does, on 30
 * October 2024) and is noise when it does not.
 *
 * The band kind is named too. A period contributes up to two rows — the slice
 * of its gains covered by basic-rate income headroom, and the rest at the
 * higher rate — and both carry the same `valid_from`. Labelled by date alone
 * they read as two rows for the same period with no stated reason to differ,
 * which on a document someone files looks like a duplicate or a date error
 * rather than the basic/higher split it actually is.
 *
 * This label is NOT money and must never be routed through formatCurrency: it
 * carries no personal figure, and hiding the rate under redaction would defeat
 * the point of showing the bands at all.
 */
function bandLabel(band: TaxBandResult, multiplePeriods: boolean): string {
  const pct = `${(Number.parseFloat(band.rate) * 100).toFixed(0)}%`
  const kind = band.rate_kind === "basic" ? "basic rate" : "higher rate"
  if (!multiplePeriods) return `Taxable gains, ${kind} @ ${pct}`
  return `Gains from ${fmtBandDate(band.valid_from)}, ${kind} @ ${pct}`
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
