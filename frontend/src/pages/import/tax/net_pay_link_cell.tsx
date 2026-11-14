import { Link2, X } from "lucide-react"
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover"
import { cn, formatDate } from "@/lib/utils"
import type { NetPayCandidate } from "@/bindings/NetPayCandidate"

/** "same day", "2 days after", "1 day before": how far a credit sits from the pay date. */
export function dayOffsetLabel(days: number): string {
  if (days === 0) return "same day"
  const n = Math.abs(days)
  return `${n} day${n === 1 ? "" : "s"} ${days > 0 ? "after" : "before"}`
}

interface Props {
  candidates: NetPayCandidate[]
  /** The accepted credit's transaction id, if any. */
  value: string | undefined
  onChange: (transactionId: string | undefined) => void
  disabled?: boolean
}

/**
 * The bank credit a payslip's net pay is linked to: an accepted chip that can
 * be removed, or a picker over the candidates the backend found.
 */
export function NetPayLinkCell({ candidates, value, onChange, disabled }: Props) {
  if (value) {
    const accepted = candidates.find((c) => c.transaction_id === value)
    return (
      <span
        data-testid="net-pay-chip"
        className="inline-flex max-w-full items-center gap-1 rounded-full border border-emerald-500/40 bg-emerald-500/10 py-0.5 pl-2 pr-1 text-xs text-emerald-700 dark:text-emerald-400"
      >
        <span className="truncate">
          {accepted
            ? `${formatDate(accepted.date)} · ${accepted.account_name} · ${accepted.amount}`
            : "Linked bank credit"}
        </span>
        <button
          type="button"
          disabled={disabled}
          onClick={() => onChange(undefined)}
          aria-label="Remove bank credit link"
          className="rounded-full p-0.5 hover:bg-emerald-500/20 disabled:opacity-50"
        >
          <X className="h-3 w-3" />
        </button>
      </span>
    )
  }
  if (candidates.length === 0) {
    return <span className="text-xs text-muted-foreground">No match</span>
  }
  return (
    <Popover>
      <PopoverTrigger
        disabled={disabled}
        className={cn(
          "inline-flex items-center gap-1 rounded-full border border-dashed px-2 py-0.5 text-xs",
          "text-muted-foreground hover:text-foreground hover:border-foreground/40 disabled:opacity-50"
        )}
      >
        <Link2 className="h-3 w-3" />
        Link bank credit ({candidates.length})
      </PopoverTrigger>
      <PopoverContent className="w-72 p-1" align="start">
        <ul className="space-y-0.5">
          {candidates.map((c) => (
            <li key={c.transaction_id}>
              <button
                type="button"
                onClick={() => onChange(c.transaction_id)}
                className="w-full rounded-md px-2 py-1.5 text-left text-xs hover:bg-accent"
              >
                <span className="flex justify-between gap-2">
                  <span className="font-medium">{formatDate(c.date)}</span>
                  <span className="tabular-nums">
                    {c.amount} {c.currency}
                  </span>
                </span>
                <span className="block truncate text-muted-foreground">
                  {c.account_name} · {dayOffsetLabel(Number(c.days_from_pay_date))}
                </span>
              </button>
            </li>
          ))}
        </ul>
      </PopoverContent>
    </Popover>
  )
}
