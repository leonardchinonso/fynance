import type { Account } from "@/types"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"
import { Currency } from "@/components/currency"
import { daysSince } from "@/lib/utils"
import { ACCOUNT_TYPE_LABELS } from "@/lib/colors"
import { AlertTriangle } from "lucide-react"

interface AccountsGridProps {
  accounts: Account[]
  onAccountClick: (accountId: string) => void
  profiles: { id: string; name: string }[]
}

export function AccountsGrid({
  accounts,
  onAccountClick,
  profiles,
}: AccountsGridProps) {
  // Group by profile
  const byProfile = new Map<string, Account[]>()
  for (const a of accounts) {
    const arr = byProfile.get(a.profile_id) ?? []
    arr.push(a)
    byProfile.set(a.profile_id, arr)
  }

  return (
    <div className="space-y-6">
      {Array.from(byProfile.entries()).map(([profileId, accs]) => {
        const profile = profiles.find((p) => p.id === profileId)
        return (
          <div key={profileId}>
            <h3 className="mb-3 text-sm font-semibold text-muted-foreground uppercase tracking-wider">
              {profile?.name ?? profileId}
            </h3>
            <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
              {accs.map((account) => (
                <AccountCard
                  key={account.id}
                  account={account}
                  onClick={() => {
                    if (account.type === "investment") {
                      onAccountClick(account.id)
                    }
                  }}
                />
              ))}
            </div>
          </div>
        )
      })}
    </div>
  )
}

function AccountCard({
  account,
  onClick,
}: {
  account: Account
  onClick: () => void
}) {
  const stale =
    account.balance_date !== null && daysSince(account.balance_date) > 30
  const isInvestment = account.type === "investment"

  return (
    <Card
      className={isInvestment ? "cursor-pointer hover:bg-muted/50 transition-colors" : ""}
      onClick={isInvestment ? onClick : undefined}
    >
      <CardHeader className="pb-2">
        <div className="flex items-center justify-between">
          <CardTitle className="text-sm font-medium">{account.name}</CardTitle>
          <Badge variant="secondary" className="text-xs capitalize">
            {ACCOUNT_TYPE_LABELS[account.type]}
          </Badge>
        </div>
        <span className="text-xs text-muted-foreground">
          {account.institution}
        </span>
      </CardHeader>
      <CardContent>
        <div className="text-xl font-semibold tabular-nums">
          <Currency amount={account.balance ?? "0"} currency={account.currency} colorize={false} />
        </div>
        <div className="mt-1 flex items-center gap-1 text-xs text-muted-foreground">
          {stale && (
            <AlertTriangle className="h-3 w-3 text-amber-500" />
          )}
          <span className={stale ? "text-amber-500" : ""}>
            Updated: {account.balance_date ?? "never"}
          </span>
        </div>
        {isInvestment && (
          <span className="mt-1 text-xs text-muted-foreground">
            Click for holdings detail
          </span>
        )}
      </CardContent>
    </Card>
  )
}
