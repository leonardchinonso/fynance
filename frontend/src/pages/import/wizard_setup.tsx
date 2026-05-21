import { useState } from "react"
import type { Account, Profile } from "@/types"
import { Button } from "@/components/ui/button"
import { DraggableList, DragHandle } from "@/components/draggable_list"
import { Building2, Eye, EyeOff, ArrowRight, ChevronDown, ChevronRight } from "lucide-react"
import { useProfileColorsContext } from "@/context/profile_colors_context"

interface Props {
  accounts: Account[]
  profiles: Profile[]
  queued: Account[]
  hidden: Account[]
  onShowAccount: (id: string) => void
  onHideAccount: (id: string) => void
  onReorder: (from: number, to: number) => void
  onContinue: () => void
  onCancel: () => void
}

function ProfileBadges({ account, profiles }: { account: Account; profiles: Profile[] }) {
  const { profileColors } = useProfileColorsContext()
  if (account.profile_ids.length === 0) return null
  const byId = new Map(profiles.map((p) => [p.id, p]))
  const matched = account.profile_ids
    .map((id) => byId.get(id))
    .filter((p): p is Profile => !!p && p.id !== "default")
  if (matched.length === 0) return null
  return (
    <>
      {matched.map((p) => {
        const color = profileColors[p.id] ?? "#78716c"
        return (
          <span
            key={p.id}
            className="inline-flex items-center text-[10px] py-0 px-1.5 h-4 font-normal rounded-full border"
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
    </>
  )
}

export function WizardSetup({
  accounts: _accounts,
  profiles,
  queued,
  hidden,
  onShowAccount,
  onHideAccount,
  onReorder,
  onContinue,
  onCancel,
}: Props) {
  const [dragId, setDragId] = useState<string | null>(null)
  // Collapse the "Won't be imported" section by default — the user is here
  // to walk through the queued accounts; the hidden list is a side concern.
  const [showHidden, setShowHidden] = useState(false)
  const draggableItems = queued.map((a) => ({ ...a, id: a.id }))
  void _accounts

  return (
    <div className="space-y-5">
      <div>
        <h2 className="text-lg font-semibold">Plan this session</h2>
        <p className="text-sm text-muted-foreground">
          Confirm which accounts you want to walk through, in what order. You can adjust here for
          this session, or change the defaults later in Settings.
        </p>
      </div>

      <section className="space-y-2">
        <header className="flex items-center justify-between">
          <h3 className="text-sm font-medium">
            You're about to import{" "}
            <span className="text-foreground font-semibold tabular-nums">{queued.length}</span>{" "}
            account{queued.length !== 1 ? "s" : ""}
          </h3>
          <span className="text-xs text-muted-foreground">drag to reorder</span>
        </header>

        {queued.length === 0 ? (
          <div className="rounded-lg border border-dashed p-4 text-center text-xs text-muted-foreground">
            Nothing queued. Add accounts from the list below to start the wizard.
          </div>
        ) : (
          <DraggableList
            items={draggableItems}
            dragId={dragId}
            onDragChange={setDragId}
            onReorder={onReorder}
            listClassName="space-y-1.5"
            renderItem={(a) => (
              <div className="flex items-center gap-2.5 rounded-lg border bg-background p-2.5">
                <DragHandle />
                <div className="flex h-8 w-8 items-center justify-center rounded-full bg-secondary shrink-0">
                  <Building2 className="h-4 w-4 text-muted-foreground" />
                </div>
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2">
                    <p className="text-sm font-medium truncate">{a.name}</p>
                    <ProfileBadges account={a} profiles={profiles} />
                  </div>
                  <p className="text-xs text-muted-foreground">
                    {a.institution} &middot; <span className="capitalize">{a.type}</span>
                  </p>
                </div>
                <Button
                  variant="ghost"
                  size="icon"
                  className="h-8 w-8 shrink-0"
                  onClick={() => onHideAccount(a.id)}
                  aria-label={`Skip ${a.name} this session`}
                  title="Skip this session"
                >
                  <Eye className="h-3.5 w-3.5" />
                </Button>
              </div>
            )}
          />
        )}
      </section>

      <section className="space-y-2">
        <button
          type="button"
          onClick={() => setShowHidden((v) => !v)}
          className="flex items-center gap-1.5 text-sm font-medium text-muted-foreground hover:text-foreground transition-colors w-full text-left"
        >
          {showHidden ? <ChevronDown className="h-3.5 w-3.5" /> : <ChevronRight className="h-3.5 w-3.5" />}
          <span>Won't be imported this session</span>
          <span className="text-muted-foreground tabular-nums">({hidden.length})</span>
        </button>

        {showHidden && (
          <p className="text-xs text-muted-foreground pl-5">
            Click an account to add it to this session.
          </p>
        )}

        {showHidden && hidden.length === 0 && (
          <div className="rounded-lg border border-dashed p-4 text-center text-xs text-muted-foreground">
            Every account is already queued.
          </div>
        )}
        {showHidden && hidden.length > 0 && (
          <div className="space-y-1.5">
            {hidden.map((a) => (
              <div
                key={a.id}
                className="flex items-center gap-2.5 rounded-lg border border-dashed p-2.5 opacity-70 hover:opacity-100 transition-opacity"
              >
                <div className="flex h-8 w-8 items-center justify-center rounded-full bg-secondary shrink-0">
                  <Building2 className="h-4 w-4 text-muted-foreground" />
                </div>
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2">
                    <p className="text-sm font-medium truncate">{a.name}</p>
                    <ProfileBadges account={a} profiles={profiles} />
                  </div>
                  <p className="text-xs text-muted-foreground">
                    {a.institution} &middot; <span className="capitalize">{a.type}</span>
                  </p>
                </div>
                <Button
                  variant="ghost"
                  size="icon"
                  className="h-8 w-8 shrink-0"
                  onClick={() => onShowAccount(a.id)}
                  aria-label={`Include ${a.name} this session`}
                  title="Include this session"
                >
                  <EyeOff className="h-3.5 w-3.5" />
                </Button>
              </div>
            ))}
          </div>
        )}
      </section>

      <div className="flex items-center justify-end gap-2 border-t pt-4">
        <Button variant="outline" onClick={onCancel}>
          Cancel
        </Button>
        <Button
          onClick={onContinue}
          disabled={queued.length === 0}
          className="bg-blue-600 text-white hover:bg-blue-600/90"
        >
          Start with {queued[0]?.name ?? "first account"}
          <ArrowRight className="ml-1 h-4 w-4" />
        </Button>
      </div>
    </div>
  )
}
