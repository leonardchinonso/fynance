import type { Account, AccountSnapshot, Currency, Profile } from "@/types"
import type { RemoteData } from "@/lib/remote_data"
import { visitRemoteData } from "@/lib/remote_data"
import type { PortfolioAccountsData } from "@/hooks/data"
import { AccountsGridSkeleton } from "@/components/skeletons"
import { AuthAwareError } from "@/components/auth_aware_error"
import { ReloadingOverlay } from "@/components/reloading_overlay"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"
import { MoneyDisplay } from "@/components/currency"
import { daysSince, formatCurrency, formatDate } from "@/lib/utils"
import { useRedactedFlag } from "@/hooks/use_redacted_flag"
import { ACCOUNT_TYPE_COLORS, ACCOUNT_TYPE_LABELS } from "@/lib/colors"
import { EmptyState } from "@/components/empty_state"
import { AlertTriangle, TrendingUp, TrendingDown } from "lucide-react"

export function AccountsGrid({
  data, profilesData, onAccountClick,
}: {
  data: RemoteData<PortfolioAccountsData>
  profilesData: RemoteData<Profile[]>
  onAccountClick: (accountId: string) => void
}) {
  return visitRemoteData(data, {
    notLoaded: () => <AccountsGridSkeleton />,
    failed: (error) => <AuthAwareError error={error} />,
    hasValue: ({ accounts, accountBalances, currencies }) => {
      const profiles = profilesData.status === "succeeded" || profilesData.status === "reloading"
        ? profilesData.value : []
      return (
        <div className="relative">
          <AccountsGridInternal
            accounts={accounts}
            onAccountClick={onAccountClick}
            profiles={profiles}
            balances={accountBalances}
            currencies={currencies}
          />
          <ReloadingOverlay active={data.status === "reloading"} />
        </div>
      )
    },
  })
}

interface AccountsGridProps {
  accounts: Account[]
  onAccountClick: (accountId: string) => void
  profiles: { id: string; name: string }[]
  startDate?: string
  balances?: AccountSnapshot[]
  currencies?: Currency[]
}

function AccountsGridInternal({
  accounts,
  onAccountClick,
  profiles,
  balances,
  currencies = [],
}: AccountsGridProps) {
  // Group by profile, with joint accounts in their own section
  const byProfile = new Map<string, Account[]>()
  for (const a of accounts) {
    if (a.profile_ids.length > 1) {
      const arr = byProfile.get("joint") ?? []
      arr.push(a)
      byProfile.set("joint", arr)
    } else {
      const pid = a.profile_ids[0] ?? "unknown"
      const arr = byProfile.get(pid) ?? []
      arr.push(a)
      byProfile.set(pid, arr)
    }
  }

  // Build FX rate lookup for converting non-preferred balances
  const preferredCurrency = currencies.find(c => c.is_preferred)?.code ?? "GBP"
  const fxRates = new Map<string, number>()
  for (const c of currencies) fxRates.set(c.code, parseFloat(c.fx_rate))
  const toPreferred = (value: number, currency: string) =>
    value * (fxRates.get(currency) ?? 1)

  // Compute delta (in account's own currency) from earliest snapshot
  const deltas = new Map<string, { value: number; currency: string }>()
  if (balances && balances.length > 0) {
    const byAccount = new Map<string, AccountSnapshot[]>()
    for (const s of balances) {
      const arr = byAccount.get(s.account_id) ?? []
      arr.push(s)
      byAccount.set(s.account_id, arr)
    }
    for (const [accId, snaps] of byAccount) {
      const sorted = [...snaps].sort((a, b) => a.as_of.localeCompare(b.as_of))
      if (sorted.length >= 2) {
        const first = parseFloat(sorted[0].balance)
        const last = parseFloat(sorted[sorted.length - 1].balance)
        deltas.set(accId, { value: last - first, currency: sorted[0].currency })
      }
    }
  }

  if (accounts.length === 0) {
    return <EmptyState />
  }

  return (
    <div className="space-y-6">
      {Array.from(byProfile.entries()).map(([groupId, accs]) => {
          const label =
            groupId === "joint"
              ? "Joint Accounts"
              : profiles.find((p) => p.id === groupId)?.name ?? groupId
          return (
            <div key={groupId}>
              <h3 className="mb-3 text-sm font-semibold text-muted-foreground uppercase tracking-wider">
                {label}
              </h3>
              <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
                {accs.map((account) => (
                  <AccountCard
                    key={account.id}
                    account={account}
                    delta={deltas.get(account.id)}
                    preferredCurrency={preferredCurrency}
                    toPreferred={toPreferred}
                    onClick={() => onAccountClick(account.id)}
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
  delta,
  preferredCurrency,
  toPreferred,
  onClick,
}: {
  account: Account
  delta?: { value: number; currency: string }
  preferredCurrency: string
  toPreferred: (value: number, currency: string) => number
  onClick: () => void
}) {
  useRedactedFlag()
  const stale =
    account.balance_date !== null && daysSince(account.balance_date) > 30
  const typeColor =
    ACCOUNT_TYPE_COLORS[account.type] ?? "#78716c"

  return (
    <Card
      className="cursor-pointer transition-all duration-200 hover:shadow-lg hover:shadow-primary/5 hover:-translate-y-0.5 hover:border-primary/20"
      onClick={onClick}
    >
      <CardHeader className="pb-2">
        <div className="flex items-center justify-between">
          <CardTitle className="text-sm font-medium">{account.name}</CardTitle>
          <Badge
            variant="secondary"
            className="text-xs capitalize"
            style={{
              borderColor: typeColor,
              color: typeColor,
            }}
          >
            {ACCOUNT_TYPE_LABELS[account.type]}
          </Badge>
        </div>
        <span className="text-xs text-muted-foreground">
          {account.institution}
        </span>
      </CardHeader>
      <CardContent>
        <div className="flex items-baseline gap-2 flex-wrap">
          <span className="text-xl font-semibold tabular-nums">
            <MoneyDisplay
              amount={account.balance ?? "0"}
              currency={account.currency}
              colorize={false}
            />
          </span>
          {account.currency !== preferredCurrency && account.balance && (
            <span className="text-xs text-muted-foreground tabular-nums">
              ({formatCurrency(
                toPreferred(parseFloat(account.balance), account.currency).toFixed(2),
                preferredCurrency
              )})
            </span>
          )}
          {delta !== undefined && delta.value !== 0 && (
            <span
              className={`flex items-center gap-0.5 text-xs font-medium ${
                delta.value >= 0 ? "text-green-500" : "text-red-500"
              }`}
            >
              {delta.value >= 0 ? (
                <TrendingUp className="h-3 w-3" />
              ) : (
                <TrendingDown className="h-3 w-3" />
              )}
              {formatCurrency(Math.abs(delta.value).toFixed(2), delta.currency)}
            </span>
          )}
        </div>
        <div className="mt-1.5 flex items-center gap-1 text-xs text-muted-foreground">
          {stale && <AlertTriangle className="h-3 w-3 text-amber-500" />}
          <span className={stale ? "text-amber-500" : ""}>
            Updated: {account.balance_date ? formatDate(account.balance_date) : "never"}
          </span>
        </div>
      </CardContent>
    </Card>
  )
}
