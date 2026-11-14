import type { ReactNode } from "react"
import { Link } from "react-router-dom"
import type { TaxYearSummary } from "@/bindings/TaxYearSummary"
import type { RemoteData } from "@/lib/remote_data"
import { visitRemoteData } from "@/lib/remote_data"
import { usePreferredCurrency } from "@/context/preferred_currency_context"
import { Badge } from "@/components/ui/badge"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Check } from "lucide-react"
import { missingMonths } from "./tax_helpers"
import { Money, SectionError } from "./tax_ui"

interface Row {
  key: string
  label: string
  detail: ReactNode
  ok: boolean
}

function StatusChip({ ok }: { ok: boolean }) {
  return ok ? (
    <Badge variant="secondary" className="gap-1 bg-emerald-500/15 text-emerald-700 dark:text-emerald-400">
      <Check className="h-3 w-3" /> OK
    </Badge>
  ) : (
    <Badge variant="destructive">Needs attention</Badge>
  )
}

function readinessRows(summary: TaxYearSummary, preferred: string): Row[] {
  const rows: Row[] = []
  for (const e of summary.employment) {
    const missing = missingMonths(summary.tax_year, e.months_covered).length
    const checkText = e.check ? `P60 ${e.check.status}` : "no P60 recorded"
    rows.push({
      key: `emp-${e.source_id}`,
      label: e.name,
      detail: `${12 - missing} of 12 months, ${checkText}`,
      ok: !!e.check && e.check.status !== "gap",
    })
  }
  for (const b of summary.self_employment) {
    const unmapped = b.bank_expenses.unmapped.length
    const checkGap = b.check?.status === "gap"
    rows.push({
      key: `biz-${b.source_id}`,
      label: b.name,
      detail: `${b.months_covered.length} month${b.months_covered.length === 1 ? "" : "s"} of sales, ${
        unmapped === 0 ? "all expense categories mapped" : `${unmapped} unmapped expense categor${unmapped === 1 ? "y" : "ies"}`
      }${checkGap ? ", accounts check has a gap" : ""}`,
      ok: b.months_covered.length > 0 && unmapped === 0 && !checkGap,
    })
  }
  const interest = summary.interest
  rows.push({
    key: "interest",
    label: "Interest",
    detail: interest.by_account.length === 0 ? "No interest totals for this tax year" : (
      <>
        {interest.by_account.length} account{interest.by_account.length === 1 ? "" : "s"}:{" "}
        <Money amount={interest.taxable_total} currency={preferred} /> taxable,{" "}
        <Money amount={interest.non_taxable_total} currency={preferred} /> tax-free
      </>
    ),
    ok: interest.by_account.length > 0,
  })
  rows.push({
    key: "gift-aid",
    label: "Gift Aid",
    detail: Number(summary.donations.gift_aid_total) === 0 ? "Nil for this tax year" : (
      <>
        <Money amount={summary.donations.gift_aid_total} currency={preferred} /> Gift Aid,{" "}
        <Money amount={summary.donations.non_gift_aid_total} currency={preferred} /> not Gift Aid
      </>
    ),
    ok: true,
  })
  return rows
}

export function ReadinessChecklist({
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
          failed: (error) => <SectionError error={error} onRetry={onRetry} />,
          hasValue: (summary) => (
            <div className="space-y-2">
              {readinessRows(summary, preferred).map((row) => (
                <div key={row.key} className="flex flex-wrap items-center gap-x-3 gap-y-1 text-sm">
                  <span className="w-36 shrink-0 truncate font-medium">{row.label}</span>
                  <span className="min-w-0 flex-1 text-muted-foreground">{row.detail}</span>
                  <StatusChip ok={row.ok} />
                </div>
              ))}
              <div className="flex flex-wrap items-center gap-x-3 text-sm">
                <span className="w-36 shrink-0 font-medium">Capital gains</span>
                <span className="min-w-0 flex-1 text-muted-foreground">
                  Disposals come from the CGT report ({summary.cgt_link.start_date} to {summary.cgt_link.end_date}).
                </span>
                <Link to="/reports/cgt" className="text-sm underline underline-offset-2">Open CGT report</Link>
              </div>
              {summary.warnings.map((w) => (
                <p key={w} className="text-xs text-muted-foreground">{w}</p>
              ))}
            </div>
          ),
        })}
      </CardContent>
    </Card>
  )
}
