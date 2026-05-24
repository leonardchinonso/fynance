import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { useNavigate } from "react-router-dom"
import { CheckCircle2, AlertTriangle, SkipForward } from "lucide-react"
import type { CommitOutcome } from "./preview/preview_review"

export interface AccountResult {
  accountId: string
  accountName: string
  outcome: CommitOutcome | null
  skipped: boolean
}

interface Props {
  accountResults: AccountResult[]
  onImportMore: () => void
}

function outcomeTotals(o: CommitOutcome) {
  return {
    transactions: o.transactionsInserted,
    holdings: o.holdingsInserted + o.holdingsUpdated,
    investments: o.investmentsInserted,
    errors: o.errors.length,
  }
}

export function ImportSummary({ accountResults, onImportMore }: Props) {
  const navigate = useNavigate()

  let totalTx = 0
  let totalHoldings = 0
  let totalInvestments = 0
  let totalErrors = 0
  for (const ar of accountResults) {
    if (!ar.outcome) continue
    const t = outcomeTotals(ar.outcome)
    totalTx += t.transactions
    totalHoldings += t.holdings
    totalInvestments += t.investments
    totalErrors += t.errors
  }

  const completedCount = accountResults.filter((a) => !a.skipped).length
  const parts: string[] = []
  if (totalTx > 0) parts.push(`${totalTx} transaction${totalTx !== 1 ? "s" : ""}`)
  if (totalHoldings > 0) parts.push(`${totalHoldings} holding${totalHoldings !== 1 ? "s" : ""}`)
  if (totalInvestments > 0) parts.push(`${totalInvestments} investment event${totalInvestments !== 1 ? "s" : ""}`)
  const summaryLine = parts.length > 0 ? parts.join(", ") + " imported" : "Nothing imported"

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-lg font-semibold">Import Complete</h2>
        <p className="text-sm text-muted-foreground">
          {summaryLine} across {completedCount} account{completedCount !== 1 ? "s" : ""}
          {totalErrors > 0 && `, ${totalErrors} error${totalErrors !== 1 ? "s" : ""}`}
        </p>
      </div>

      <div className="space-y-2">
        {accountResults.map((ar) => {
          const totals = ar.outcome ? outcomeTotals(ar.outcome) : null
          const hasErrors = totals && totals.errors > 0
          return (
            <div key={ar.accountId} className="rounded-lg border p-3">
              <div className="flex items-center gap-2">
                {ar.skipped ? (
                  <SkipForward className="h-4 w-4 text-amber-500 shrink-0" />
                ) : hasErrors ? (
                  <AlertTriangle className="h-4 w-4 text-amber-500 shrink-0" />
                ) : (
                  <CheckCircle2 className="h-4 w-4 text-green-600 shrink-0" />
                )}
                <p className="text-sm font-medium flex-1">{ar.accountName}</p>
                {ar.skipped ? (
                  <Badge variant="outline" className="text-xs">Skipped</Badge>
                ) : totals ? (
                  <div className="flex gap-3 text-xs text-muted-foreground tabular-nums">
                    {totals.transactions > 0 && (
                      <span>{totals.transactions} {totals.transactions === 1 ? "transaction" : "transactions"}</span>
                    )}
                    {totals.holdings > 0 && (
                      <span>{totals.holdings} {totals.holdings === 1 ? "holding" : "holdings"}</span>
                    )}
                    {totals.investments > 0 && (
                      <span>{totals.investments} {totals.investments === 1 ? "investment" : "investments"}</span>
                    )}
                  </div>
                ) : null}
              </div>
            </div>
          )
        })}
      </div>

      <div className="flex gap-2">
        <Button variant="outline" onClick={onImportMore}>Import More</Button>
        <Button onClick={() => navigate("/transactions")}>View Transactions</Button>
      </div>
    </div>
  )
}
