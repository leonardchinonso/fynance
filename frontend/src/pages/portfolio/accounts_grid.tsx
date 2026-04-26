import { useState } from "react"
import type { Account, AccountSnapshot, Profile } from "@/types"
import type { RemoteData } from "@/lib/remote_data"
import { visitRemoteData } from "@/lib/remote_data"
import type { PortfolioPageData } from "@/hooks/data"
import { AccountsGridSkeleton } from "@/components/skeletons"
import { NonIdealState } from "@/components/non_ideal_state"
import { ReloadingOverlay } from "@/components/reloading_overlay"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"
import { Currency } from "@/components/currency"
import { daysSince, formatCurrency, formatDate } from "@/lib/utils"
import { ACCOUNT_TYPE_COLORS, ACCOUNT_TYPE_LABELS } from "@/lib/colors"
import { EmptyState } from "@/components/empty_state"
import { AlertTriangle, TrendingUp, TrendingDown } from "lucide-react"
import {
  Sheet, SheetContent, SheetHeader, SheetTitle,
} from "@/components/ui/sheet"

export function AccountsGrid({
  data, profilesData, onAccountClick,
}: {
  data: RemoteData<PortfolioPageData>
  profilesData: RemoteData<Profile[]>
  onAccountClick: (accountId: string) => void
}) {
  return visitRemoteData(data, {
    notLoaded: () => <AccountsGridSkeleton />,
    failed: (error) => <NonIdealState title="Could not load accounts" description={error} />,
    hasValue: ({ portfolio, accountBalances }) => {
      const profiles = profilesData.status === "succeeded" || profilesData.status === "reloading"
        ? profilesData.value : []
      return (
        <div className="relative">
          <AccountsGridInternal
            accounts={portfolio.accounts}
            onAccountClick={onAccountClick}
            profiles={profiles}
            balances={accountBalances}
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
}

function AccountsGridInternal({
  accounts,
  onAccountClick,
  profiles,
  balances,
}: AccountsGridProps) {
  const [selectedNonInvestment, setSelectedNonInvestment] = useState<Account | null>(null)

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

  // Compute delta from earliest snapshot for each account
  const deltas = new Map<string, number>()
  if (balances && balances.length > 0) {
    const byAccount = new Map<string, AccountSnapshot[]>()
    for (const s of balances) {
      const arr = byAccount.get(s.account_id) ?? []
      arr.push(s)
      byAccount.set(s.account_id, arr)
    }
    for (const [accId, snaps] of byAccount) {
      const sorted = [...snaps].sort((a, b) =>
        a.as_of.localeCompare(b.as_of)
      )
      if (sorted.length >= 2) {
        const first = parseFloat(sorted[0].balance)
        const last = parseFloat(sorted[sorted.length - 1].balance)
        deltas.set(accId, last - first)
      }
    }
  }

  if (accounts.length === 0) {
    return <EmptyState />
  }

  return (
    <>
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
                    onClick={() => {
                      if (account.type === "investment" || account.type === "pension") {
                        onAccountClick(account.id)
                      } else {
                        setSelectedNonInvestment(account)
                      }
                    }}
                  />
                ))}
              </div>
            </div>
          )
        })}
      </div>

      {/* Non-investment account detail sheet */}
      <AccountDetailSheet
        account={selectedNonInvestment}
        onClose={() => setSelectedNonInvestment(null)}
      />
    </>
  )
}

function AccountCard({
  account,
  delta,
  onClick,
}: {
  account: Account
  delta?: number
  onClick: () => void
}) {
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
        <div className="flex items-baseline gap-2">
          <span className="text-xl font-semibold tabular-nums">
            <Currency
              amount={account.balance ?? "0"}
              currency={account.currency}
              colorize={false}
            />
          </span>
          {delta !== undefined && delta !== 0 && (
            <span
              className={`flex items-center gap-0.5 text-xs font-medium ${
                delta >= 0 ? "text-green-500" : "text-red-500"
              }`}
            >
              {delta >= 0 ? (
                <TrendingUp className="h-3 w-3" />
              ) : (
                <TrendingDown className="h-3 w-3" />
              )}
              {formatCurrency(Math.abs(delta).toFixed(2))}
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

function AccountDetailSheet({
  account,
  onClose,
}: {
  account: Account | null
  onClose: () => void
}) {
  if (!account) return null

  return (
    <Sheet open={!!account} onOpenChange={() => onClose()}>
      <SheetContent className="w-full sm:max-w-md overflow-y-auto px-6">
        <SheetHeader className="pb-4">
          <SheetTitle>{account.name}</SheetTitle>
        </SheetHeader>
        <div className="space-y-4">
          <div className="space-y-2">
            <DetailRow label="Institution" value={account.institution} />
            <DetailRow
              label="Type"
              value={ACCOUNT_TYPE_LABELS[account.type]}
            />
            <DetailRow label="Currency" value={account.currency} />
            <DetailRow
              label="Balance"
              value={formatCurrency(account.balance ?? "0", account.currency)}
            />
            <DetailRow
              label="Last Updated"
              value={account.balance_date ? formatDate(account.balance_date) : "Never"}
            />
            {account.notes && (
              <DetailRow label="Notes" value={account.notes} />
            )}
          </div>
        </div>
      </SheetContent>
    </Sheet>
  )
}

function DetailRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex justify-between py-1.5 border-b border-border/50">
      <span className="text-sm text-muted-foreground">{label}</span>
      <span className="text-sm font-medium">{value}</span>
    </div>
  )
}
