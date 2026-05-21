import { cn } from "@/lib/utils"
import type { Account } from "@/types"
import { Check, Circle, SkipForward } from "lucide-react"

interface Props {
  accounts: Account[]
  currentIndex: number
  completedIds: Set<string>
  skippedIds: Set<string>
  /** Click to revisit an earlier account. Forward jumps are disallowed. */
  onSelectAccount: (account: Account) => void
}

export function WizardProgress({ accounts, currentIndex, completedIds, skippedIds, onSelectAccount }: Props) {
  return (
    <div className="space-y-1">
      <p className="text-xs font-medium text-muted-foreground uppercase tracking-wider mb-2">
        Account {Math.min(currentIndex + 1, accounts.length)} of {accounts.length}
      </p>
      <div className="space-y-0.5">
        {accounts.map((account, idx) => {
          const completed = completedIds.has(account.id)
          const skipped = skippedIds.has(account.id)
          const current = idx === currentIndex
          const navigable = idx < currentIndex
          const rowClass = cn(
            "flex items-center gap-2 rounded-md px-2 py-1.5 text-sm text-left w-full transition-colors",
            current && "bg-blue-500/15 border border-blue-500/40 font-medium text-blue-500",
            !current && !completed && !skipped && "text-muted-foreground",
            navigable && "hover:bg-secondary/50 cursor-pointer",
            !navigable && !current && "cursor-default"
          )
          const inner = (
            <>
              {completed ? (
                <Check className="h-3.5 w-3.5 text-green-600 shrink-0" />
              ) : skipped ? (
                <SkipForward className="h-3.5 w-3.5 text-amber-500 shrink-0" />
              ) : (
                <Circle className={cn("h-3.5 w-3.5 shrink-0", current ? "text-primary" : "text-muted-foreground/30")} />
              )}
              <span className="truncate">{account.name}</span>
            </>
          )
          return navigable ? (
            <button
              key={account.id}
              type="button"
              onClick={() => onSelectAccount(account)}
              className={rowClass}
            >
              {inner}
            </button>
          ) : (
            <div key={account.id} className={rowClass} aria-current={current ? "step" : undefined}>
              {inner}
            </div>
          )
        })}
      </div>
    </div>
  )
}
