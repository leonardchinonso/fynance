import { Button } from "@/components/ui/button"
import { CountBadge } from "@/components/count_badge"
import { X, FileText, History } from "lucide-react"
import { cn } from "@/lib/utils"
import type { Account, Profile } from "@/types"
import type { RecentImportEntry } from "@/hooks/use_recent_imports"
import { useProfileColorsContext } from "@/context/profile_colors_context"
import { accountTypeClasses } from "@/lib/account_type_colors"

interface Props {
  entries: RecentImportEntry[]
  accounts: Account[]
  profiles: Profile[]
  onResume: (entry: RecentImportEntry) => void
  onDiscard: (id: string) => void
}

function formatTimestamp(ms: number): string {
  const d = new Date(ms)
  const now = new Date()
  const sameDay =
    d.getFullYear() === now.getFullYear() &&
    d.getMonth() === now.getMonth() &&
    d.getDate() === now.getDate()
  const time = d.toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" })
  if (sameDay) return `Today, ${time}`
  const yesterday = new Date(now)
  yesterday.setDate(now.getDate() - 1)
  const isYesterday =
    d.getFullYear() === yesterday.getFullYear() &&
    d.getMonth() === yesterday.getMonth() &&
    d.getDate() === yesterday.getDate()
  if (isYesterday) return `Yesterday, ${time}`
  return d.toLocaleDateString(undefined, { day: "numeric", month: "short" }) + `, ${time}`
}

function summarize(entry: RecentImportEntry): string {
  const txCount = (entry.edits.txPayload?.transactions.length ?? 0) - entry.edits.txDeleted.length
  const hldCount =
    (entry.edits.holdingsPayload?.holdings.length ?? 0) - entry.edits.holdingsDeleted.length
  const invCount = (entry.edits.invPayload?.events.length ?? 0) - entry.edits.invDeleted.length
  const parts: string[] = []
  if (txCount > 0) parts.push(`${txCount} transaction${txCount !== 1 ? "s" : ""}`)
  if (hldCount > 0) parts.push(`${hldCount} holding${hldCount !== 1 ? "s" : ""}`)
  if (invCount > 0) parts.push(`${invCount} investment event${invCount !== 1 ? "s" : ""}`)
  return parts.join(" · ") || "Nothing to commit"
}

export function RecentImportsList({ entries, accounts, profiles, onResume, onDiscard }: Props) {
  const { profileColors } = useProfileColorsContext()
  if (entries.length === 0) return null

  const accountById = new Map(accounts.map((a) => [a.id, a]))
  const profileById = new Map(profiles.map((p) => [p.id, p]))

  return (
    <div className="mt-6 space-y-2">
      <div className="flex items-center gap-2 px-1">
        <History className="h-3.5 w-3.5 text-muted-foreground" />
        <p className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
          Recent imports
        </p>
        <CountBadge count={entries.length} />
      </div>

      <ul className="space-y-1.5">
        {entries.map((entry) => {
          const account = accountById.get(entry.accountId)
          const accountLabel = account ? account.name : entry.accountId
          return (
            <li key={entry.id}>
              <div className="group flex items-center gap-2 rounded-lg border bg-card p-3 hover:border-foreground/30 transition-colors">
                <button
                  onClick={() => onResume(entry)}
                  className="flex flex-1 items-center gap-3 text-left min-w-0 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50 rounded-md -m-1 p-1"
                >
                  <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-secondary">
                    <FileText className="h-4 w-4 text-muted-foreground" />
                  </div>
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2 flex-wrap">
                      <p className="truncate text-sm font-medium">{accountLabel}</p>
                      {account && (
                        <span
                          className={cn(
                            "inline-flex items-center text-[10px] py-0 px-1.5 h-4 font-normal rounded-full border shrink-0 capitalize",
                            accountTypeClasses(account.type)
                          )}
                        >
                          {account.type}
                        </span>
                      )}
                      {account?.profile_ids
                        .map((id) => profileById.get(id))
                        .filter((p): p is Profile => !!p && p.id !== "default")
                        .map((p) => {
                          const color = profileColors[p.id] ?? "#78716c"
                          return (
                            <span
                              key={p.id}
                              className="inline-flex items-center text-[10px] py-0 px-1.5 h-4 font-normal rounded-full border shrink-0"
                              style={{
                                backgroundColor: `${color}24`,
                                borderColor: `${color}66`,
                                color,
                              }}
                            >
                              {p.name}
                            </span>
                          )
                        })}
                    </div>
                    <p className="truncate text-xs text-muted-foreground">
                      {formatTimestamp(entry.timestamp)} · {summarize(entry)}
                    </p>
                    {entry.fileNames.length > 0 && (
                      <p className="truncate text-[11px] text-muted-foreground/70">
                        {entry.fileNames.join(", ")}
                      </p>
                    )}
                  </div>
                </button>
                <Button
                  variant="ghost"
                  size="icon"
                  className="h-7 w-7 shrink-0 opacity-60 group-hover:opacity-100"
                  onClick={(e) => {
                    e.stopPropagation()
                    onDiscard(entry.id)
                  }}
                  aria-label="Discard this recent import"
                  title="Discard"
                >
                  <X className="h-3.5 w-3.5" />
                </Button>
              </div>
            </li>
          )
        })}
      </ul>
    </div>
  )
}
