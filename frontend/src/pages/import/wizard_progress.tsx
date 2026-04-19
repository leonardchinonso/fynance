import { cn } from "@/lib/utils"
import type { Account } from "@/types"
import { Check, Circle, SkipForward } from "lucide-react"

interface Props {
  accounts: Account[]
  currentIndex: number
  completedIds: Set<string>
  skippedIds: Set<string>
}

export function WizardProgress({ accounts, currentIndex, completedIds, skippedIds }: Props) {
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
          return (
            <div
              key={account.id}
              className={cn(
                "flex items-center gap-2 rounded-md px-2 py-1.5 text-sm transition-colors",
                current && "bg-secondary font-medium",
                !current && !completed && !skipped && "text-muted-foreground"
              )}
            >
              {completed ? (
                <Check className="h-3.5 w-3.5 text-green-600 shrink-0" />
              ) : skipped ? (
                <SkipForward className="h-3.5 w-3.5 text-amber-500 shrink-0" />
              ) : (
                <Circle className={cn("h-3.5 w-3.5 shrink-0", current ? "text-primary" : "text-muted-foreground/30")} />
              )}
              <span className="truncate">{account.name}</span>
            </div>
          )
        })}
      </div>
    </div>
  )
}
