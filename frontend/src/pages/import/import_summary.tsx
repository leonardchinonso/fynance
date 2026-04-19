import type { ImportResult } from "@/types"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { useNavigate } from "react-router-dom"
import { CheckCircle2, AlertTriangle, SkipForward } from "lucide-react"

interface AccountResult {
  accountId: string
  accountName: string
  results: ImportResult[]
  skipped: boolean
}

interface Props {
  accountResults: AccountResult[]
  onImportMore: () => void
}

function bigintToNumber(val: bigint): number {
  return Number(val)
}

export function ImportSummary({ accountResults, onImportMore }: Props) {
  const navigate = useNavigate()

  const totalInserted = accountResults.reduce((sum, ar) =>
    sum + ar.results.reduce((s, r) => s + bigintToNumber(r.rows_inserted), 0), 0)
  const totalErrors = accountResults.reduce((sum, ar) =>
    sum + ar.results.reduce((s, r) => s + r.errors.length, 0), 0)

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-lg font-semibold">Import Complete</h2>
        <p className="text-sm text-muted-foreground">
          {totalInserted} transaction{totalInserted !== 1 ? "s" : ""} imported across {accountResults.filter((a) => !a.skipped).length} account{accountResults.filter((a) => !a.skipped).length !== 1 ? "s" : ""}
          {totalErrors > 0 && `, ${totalErrors} error${totalErrors !== 1 ? "s" : ""}`}
        </p>
      </div>

      <div className="space-y-2">
        {accountResults.map((ar) => (
          <div key={ar.accountId} className="rounded-lg border p-3">
            <div className="flex items-center gap-2">
              {ar.skipped ? (
                <SkipForward className="h-4 w-4 text-amber-500 shrink-0" />
              ) : ar.results.some((r) => r.errors.length > 0) ? (
                <AlertTriangle className="h-4 w-4 text-amber-500 shrink-0" />
              ) : (
                <CheckCircle2 className="h-4 w-4 text-green-600 shrink-0" />
              )}
              <p className="text-sm font-medium flex-1">{ar.accountName}</p>
              {ar.skipped ? (
                <Badge variant="outline" className="text-xs">Skipped</Badge>
              ) : (
                <div className="flex gap-2 text-xs text-muted-foreground">
                  {ar.results.map((r, i) => (
                    <span key={i} className="tabular-nums">
                      {bigintToNumber(r.rows_inserted)} new, {bigintToNumber(r.rows_duplicate)} dup
                    </span>
                  ))}
                </div>
              )}
            </div>
          </div>
        ))}
      </div>

      <div className="flex gap-2">
        <Button variant="outline" onClick={onImportMore}>Import More</Button>
        <Button onClick={() => navigate("/transactions")}>View Transactions</Button>
      </div>
    </div>
  )
}
