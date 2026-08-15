import { useState } from "react"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Button } from "@/components/ui/button"
import { ChevronDown, ChevronRight } from "lucide-react"
import { formatCurrency } from "@/lib/utils"
import { useRedactedFlag } from "@/hooks/use_redacted_flag"
import type { S104PoolState } from "@/bindings/S104PoolState"

interface CgtPoolWorkingsProps {
  pools: S104PoolState[]
}

export function CgtPoolWorkings({ pools }: CgtPoolWorkingsProps) {
  useRedactedFlag()
  const [open, setOpen] = useState(false)
  const nonEmpty = pools.filter((p) => Number.parseFloat(p.current_shares) > 0)
  if (nonEmpty.length === 0) return null

  return (
    <Card>
      <CardHeader className="flex-row items-center justify-between space-y-0">
        <CardTitle>S104 pool workings</CardTitle>
        <Button variant="ghost" size="sm" onClick={() => setOpen((v) => !v)}>
          {open ? <ChevronDown className="h-4 w-4" /> : <ChevronRight className="h-4 w-4" />}
          <span className="ml-1">
            {open ? "Hide" : "Show"} {nonEmpty.length} {nonEmpty.length === 1 ? "pool" : "pools"}
          </span>
        </Button>
      </CardHeader>
      {open && (
        <CardContent className="grid gap-3 sm:grid-cols-2">
          {nonEmpty.map((p) => {
            // Pool figures are held in the symbol's own currency, which the pool now
            // states outright. This used to be looked up from the disposals in the
            // window and fell back to the base currency when a symbol had none — so a
            // pool with no disposals this period was labelled in the wrong currency.
            const cur = p.original_currency
            return (
              <div key={p.symbol} className="rounded-md border bg-card p-4">
                <h3 className="text-sm font-semibold">{p.symbol}</h3>
                <dl className="mt-2 space-y-1 text-sm">
                  <Pair label="Current shares" value={fmtShares(p.current_shares)} />
                  <Pair
                    label="Total allowable expenditure"
                    value={formatCurrency(p.total_allowable_expenditure, cur)}
                  />
                  <Pair
                    label="Average cost per share"
                    value={formatCurrency(p.average_cost_per_share, cur)}
                  />
                </dl>
              </div>
            )
          })}
        </CardContent>
      )}
    </Card>
  )
}

function fmtShares(qty: string): string {
  const n = Number.parseFloat(qty)
  if (!Number.isFinite(n)) return "—"
  return n.toLocaleString("en-GB", { minimumFractionDigits: 0, maximumFractionDigits: 4 })
}

function Pair({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-baseline justify-between gap-4">
      <dt className="text-muted-foreground">{label}</dt>
      <dd className="tabular-nums">{value}</dd>
    </div>
  )
}
