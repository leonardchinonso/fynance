import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { formatCurrency } from "@/lib/utils"
import { usePreferredCurrency, useCurrenciesFromContext } from "@/context/preferred_currency_context"
import type { EstimatedPrice } from "@/bindings/EstimatedPrice"
import type { ParserCallCost } from "@/bindings/ParserCallCost"

interface Props {
  price?: EstimatedPrice | null
  className?: string
}

function formatDuration(ms: number): string {
  if (!Number.isFinite(ms) || ms <= 0) return "—"
  if (ms < 1000) return `${ms}ms`
  const totalSec = Math.round(ms / 1000)
  const m = Math.floor(totalSec / 60)
  const s = totalSec % 60
  if (m === 0) return `${s}s`
  return `${m}m ${s}s`
}

export function CostTag({ price, className }: Props) {
  const preferred = usePreferredCurrency()
  const currencies = useCurrenciesFromContext()

  if (!price || !price.calls || price.calls.length === 0) return null

  const totalUsd = parseFloat(price.total)
  if (Number.isNaN(totalUsd) || totalUsd <= 0) return null

  const usdToPreferred = (usd: number): number => {
    if (preferred === "USD") return usd
    const usdEntry = currencies.find((c) => c.code === "USD")
    if (!usdEntry) return usd
    const rate = parseFloat(usdEntry.fx_rate)
    if (Number.isNaN(rate) || rate <= 0) return usd
    return usd * rate
  }

  const totalPreferred = usdToPreferred(totalUsd)
  const totalDisplay = formatCurrency(totalPreferred.toFixed(2), preferred)

  return (
    <Tooltip>
      <TooltipTrigger
        className={
          "inline-flex items-center rounded-full border bg-secondary/50 px-2 py-0.5 text-[10px] font-medium text-muted-foreground tabular-nums cursor-default " +
          (className ?? "")
        }
      >
        {totalDisplay}
      </TooltipTrigger>
      <TooltipContent
        side="top"
        // Override the default inverted pill — unreadable for a table.
        className="max-w-sm bg-popover text-popover-foreground ring-1 ring-foreground/10 px-3 py-2"
      >
        <div className="space-y-1.5 text-xs">
          <p className="text-[10px] text-muted-foreground">
            USD; displayed value converted at the current FX rate.
          </p>
          <table className="w-full">
            <thead>
              <tr className="text-left text-[10px] text-muted-foreground">
                <th className="pr-2">Parser</th>
                <th className="pr-2">Agent</th>
                <th className="pr-2 text-right">USD</th>
                <th className="text-right">Time</th>
              </tr>
            </thead>
            <tbody>
              {price.calls.map((c: ParserCallCost, i) => (
                <tr key={`${c.parser}-${i}`} className="tabular-nums">
                  <td className="pr-2 font-mono text-[10px]">{c.parser}</td>
                  <td className="pr-2 text-[10px]">{c.agent}</td>
                  <td className="pr-2 text-right">${parseFloat(c.amount).toFixed(4)}</td>
                  <td className="text-right text-muted-foreground">
                    {formatDuration(Number(c.duration_ms))}
                  </td>
                </tr>
              ))}
              <tr className="border-t font-medium tabular-nums">
                <td className="pr-2 pt-1" colSpan={2}>
                  Total
                </td>
                <td className="pr-2 pt-1 text-right">${totalUsd.toFixed(4)}</td>
                <td className="pt-1 text-right text-muted-foreground">
                  {formatDuration(
                    price.calls.reduce((s, c) => s + Number(c.duration_ms), 0)
                  )}
                </td>
              </tr>
            </tbody>
          </table>
        </div>
      </TooltipContent>
    </Tooltip>
  )
}
