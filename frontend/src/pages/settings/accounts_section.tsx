import { useState, type ReactNode } from "react"
import { api } from "@/api/client"
import type { Account, Profile, AccountType } from "@/types"
import type { RemoteData } from "@/lib/remote_data"
import { visitRemoteData } from "@/lib/remote_data"
import { useIngestionPreferences } from "@/hooks/use_ingestion_preferences"
import { useProfileColorsContext } from "@/context/profile_colors_context"
import { useCurrenciesFromContext } from "@/context/preferred_currency_context"
import { DraggableList, DragHandle } from "@/components/draggable_list"
import { SettingsListSkeleton } from "@/components/skeletons"
import { AuthAwareError } from "@/components/auth_aware_error"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { Switch } from "@/components/ui/switch"
import {
  DropdownMenu, DropdownMenuTrigger, DropdownMenuContent, DropdownMenuItem,
} from "@/components/ui/dropdown-menu"
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover"
import {
  Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList,
} from "@/components/ui/command"
import { Trash2, Pencil, Plus, Building2, Eye, EyeOff, Check, ChevronsUpDown, CircleHelp } from "lucide-react"
import { Tooltip, TooltipContent, TooltipTrigger, TooltipProvider } from "@/components/ui/tooltip"
import { cn, formatCurrency } from "@/lib/utils"
import { MoneyDisplay } from "@/components/currency"
import { ConfirmDialog } from "@/components/confirm_dialog"
import { accountTypeClasses } from "@/lib/account_type_colors"
import { ACCOUNT_TYPE_COLORS, ACCOUNT_TYPE_LABELS } from "@/lib/colors"

// Account type tags. Investment + ISA are condensed into one "Investments" tag
// with an ISA / non-ISA submenu; everything else is a single tag. Order keeps
// the everyday-cash types first, then investments, then long-term assets.
type AccountTypeTag =
  | { kind: "single"; type: AccountType; label: string }
  | { kind: "group"; key: string; label: string; types: AccountType[] }

const ACCOUNT_TYPE_TAGS: AccountTypeTag[] = [
  { kind: "single", type: "checking", label: "Checking" },
  { kind: "single", type: "savings", label: "Savings" },
  { kind: "single", type: "emergency_fund", label: "Emergency fund" },
  { kind: "single", type: "credit", label: "Credit" },
  { kind: "group", key: "investments", label: "Investments", types: ["investment", "investment_isa"] },
  { kind: "single", type: "pension", label: "Pension" },
  { kind: "single", type: "property", label: "Property" },
]

// Rows for the Type help tooltip: [type, label, one-line description].
const ACCOUNT_TYPE_HELP: [AccountType, string, string][] = [
  ["checking", "Checking", "everyday current account"],
  ["savings", "Savings", "savings balance"],
  ["emergency_fund", "Emergency fund", "rainy-day cash"],
  ["credit", "Credit", "card; a liability"],
  ["investment", "Investments", "brokerage; ISA vs non-ISA affects CGT (ISA tax-free)"],
  ["pension", "Pension", "retirement savings"],
  ["property", "Property", "property value"],
]

interface Props {
  data: RemoteData<Account[]>
  profilesData: RemoteData<Profile[]>
  onRefresh: () => void
}

export function AccountsSection({ data, profilesData, onRefresh }: Props) {
  const profiles = profilesData.status === "succeeded" || profilesData.status === "reloading"
    ? profilesData.value : []
  // Flat list is the default; toggle exposes the import-wizard config view
  // (drag-to-reorder + per-account visibility split).
  const [wizardMode, setWizardMode] = useState(false)

  return (
    <Card id="accounts">
      <CardHeader>
        <div className="flex items-center justify-between gap-3 flex-wrap">
          <CardTitle className="text-lg">Accounts</CardTitle>
          <div className="flex items-center gap-3">
            <label className="flex items-center gap-2 text-xs text-muted-foreground cursor-pointer">
              <span>Import wizard config</span>
              <Switch checked={wizardMode} onCheckedChange={(v) => setWizardMode(!!v)} size="sm" />
            </label>
            {(data.status === "succeeded" || data.status === "reloading") && (
              <AddAccountButton profiles={profiles} onRefresh={onRefresh} />
            )}
          </div>
        </div>
        <p className="text-sm text-muted-foreground">
          {wizardMode
            ? "Drag to set the import wizard order. Toggle visibility to include or exclude from the wizard."
            : "Bank accounts, investment accounts, credit cards, and other financial accounts."}
        </p>
      </CardHeader>
      <CardContent>
        {visitRemoteData(data, {
          notLoaded: () => <SettingsListSkeleton rows={4} />,
          failed: (error) => <AuthAwareError error={error} onRetry={onRefresh} />,
          hasValue: (accounts) => <AccountsList accounts={accounts} profiles={profiles} onRefresh={onRefresh} wizardMode={wizardMode} />,
        })}
      </CardContent>
    </Card>
  )
}

/** Profile chips shown next to an account name, colored by each profile's color. */
function ProfileTags({ profileIds, profiles }: { profileIds: string[]; profiles: Profile[] }) {
  const { profileColors } = useProfileColorsContext()
  if (profileIds.length === 0) return null
  return (
    <span className="flex items-center gap-1">
      {profileIds.map((id) => {
        const p = profiles.find((x) => x.id === id)
        if (!p) return null
        const color = profileColors[id] ?? "#78716c"
        return (
          <span
            key={id}
            className="inline-flex items-center rounded-full border px-1.5 h-4 text-[10px] font-medium"
            style={{ color, borderColor: `${color}66`, backgroundColor: `${color}1f` }}
          >
            {p.name}
          </span>
        )
      })}
    </span>
  )
}

/**
 * Account balance: native amount with the preferred-currency equivalent inline
 * (muted) when the account isn't already in the preferred currency. From sm up
 * it's a fixed 160px right-aligned column so the type badges line up across
 * rows; below sm it's a full-width left-aligned row in the stacked card layout.
 */
function AccountBalance({ account, preferredCurrency, fxRate }: {
  account: Account
  preferredCurrency: string
  fxRate?: string
}) {
  if (!account.balance) return null
  const foreign = account.currency !== preferredCurrency
  // Only show a converted figure when we actually have a usable rate; otherwise
  // showing balance × 1 would fabricate a bogus 1:1 conversion.
  const rate = fxRate != null ? parseFloat(fxRate) : NaN
  const converted = foreign && Number.isFinite(rate) && rate > 0
    ? formatCurrency((parseFloat(account.balance) * rate).toFixed(2), preferredCurrency)
    : null
  return (
    <p className="text-sm font-medium tabular-nums sm:w-40 sm:text-right sm:whitespace-nowrap">
      <span className="inline-flex items-baseline gap-1.5">
        {converted && <span className="text-xs font-normal text-muted-foreground">{converted}</span>}
        <MoneyDisplay amount={account.balance} currency={account.currency} colorize={false} />
      </span>
    </p>
  )
}

/** The account-type badge, shown on the right of a row before the balance. */
function TypeBadge({ type }: { type: AccountType }) {
  return (
    <span className={cn("inline-flex items-center text-[10px] py-0 px-1.5 h-4 font-normal rounded-full border shrink-0", accountTypeClasses(type))}>
      {ACCOUNT_TYPE_LABELS[type]}
    </span>
  )
}

/**
 * One account list row: icon, identity, type badge, balance, and caller-supplied
 * leading (e.g. a drag handle) and trailing (action buttons) controls. Below sm
 * the identity, badge, and balance stack into three rows so nothing is crushed;
 * from sm up they sit inline with the badge and balance right of the name.
 */
function AccountRow({
  account, profiles, preferredCurrency, fxRate, showBalance = true, dashed = false, leading, trailing,
}: {
  account: Account
  profiles: Profile[]
  preferredCurrency: string
  fxRate?: string
  showBalance?: boolean
  dashed?: boolean
  leading?: ReactNode
  trailing: ReactNode
}) {
  return (
    <div className={cn(
      "flex items-center gap-3 rounded-lg border p-3 group",
      dashed ? "border-dashed opacity-60 hover:opacity-100 transition-opacity" : "bg-background",
    )}>
      {leading}
      <div className="flex h-8 w-8 items-center justify-center rounded-full bg-secondary shrink-0">
        <Building2 className="h-4 w-4 text-muted-foreground" />
      </div>
      <div className="flex-auto min-w-0 flex flex-col gap-1 sm:flex-row sm:items-center sm:gap-3">
        <div className="min-w-0 sm:flex-auto">
          <div className="flex items-center gap-2 min-w-0">
            <p className="text-sm font-medium truncate">{account.name}</p>
            <ProfileTags profileIds={account.profile_ids ?? []} profiles={profiles} />
          </div>
          <div className="flex items-center gap-2 min-w-0">
            <p className="text-xs text-muted-foreground truncate">{account.institution} &middot; {account.currency}</p>
            {/* On stacked rows the type badge rides with the institution line. */}
            <span className="sm:hidden shrink-0"><TypeBadge type={account.type} /></span>
          </div>
        </div>
        {/* On inline rows the type badge sits before the balance. */}
        <span className="hidden sm:inline-flex shrink-0"><TypeBadge type={account.type} /></span>
        {showBalance && (
          <AccountBalance account={account} preferredCurrency={preferredCurrency} fxRate={fxRate} />
        )}
      </div>
      {trailing}
    </div>
  )
}

function AccountsList({ accounts, profiles, onRefresh, wizardMode }: { accounts: Account[]; profiles: Profile[]; onRefresh: () => void; wizardMode: boolean }) {
  const [dragId, setDragId] = useState<string | null>(null)
  const [editing, setEditing] = useState<Account | null>(null)
  const [deleting, setDeleting] = useState<Account | null>(null)
  const [deleteBusy, setDeleteBusy] = useState(false)
  const [deleteError, setDeleteError] = useState<string | null>(null)
  const {
    getOrderedAccounts,
    getHiddenAccounts,
    showAccount,
    hideAccount,
    reorderAccounts,
  } = useIngestionPreferences()

  const currencies = useCurrenciesFromContext()
  const preferredCurrency = currencies.find((c) => c.is_preferred)?.code ?? "GBP"
  const fxRates = new Map(currencies.map((c) => [c.code, c.fx_rate]))
  const fxRateFor = (currency: string) => fxRates.get(currency)

  function requestDelete(account: Account) {
    setDeleteError(null)
    setDeleting(account)
  }

  async function handleDeleteConfirm() {
    if (!deleting) return
    setDeleteBusy(true)
    setDeleteError(null)
    try {
      await api.deleteAccount(deleting.id)
      setDeleting(null)
      onRefresh()
    } catch (err) {
      setDeleteError(err instanceof Error ? err.message : String(err))
    } finally {
      setDeleteBusy(false)
    }
  }

  if (accounts.length === 0) return (
    <p className="text-sm text-muted-foreground py-4 text-center">No accounts yet.</p>
  )

  // Flat-list view: no drag, no visibility toggle, just edit + delete.
  if (!wizardMode) {
    return (
      <>
        <div className="space-y-2">
          {accounts.map((a) => (
            <AccountRow
              key={a.id}
              account={a}
              profiles={profiles}
              preferredCurrency={preferredCurrency}
              fxRate={fxRateFor(a.currency)}
              trailing={
                <>
                  <Button variant="ghost" size="icon" className="h-8 w-8 shrink-0" onClick={() => setEditing(a)} title="Edit account">
                    <Pencil className="h-3.5 w-3.5" />
                  </Button>
                  <Button variant="ghost" size="icon" className="h-8 w-8 shrink-0" onClick={() => requestDelete(a)} title="Delete account">
                    <Trash2 className="h-3.5 w-3.5" />
                  </Button>
                </>
              }
            />
          ))}
        </div>

        {editing && (
          <EditAccountDialog
            account={editing}
            profiles={profiles}
            onClose={() => setEditing(null)}
            onSaved={() => { setEditing(null); onRefresh() }}
          />
        )}

        <ConfirmDialog
          open={!!deleting}
          onOpenChange={(open) => { if (!open) setDeleting(null) }}
          title="Delete account?"
          busy={deleteBusy}
          error={deleteError}
          onConfirm={handleDeleteConfirm}
        >
          This deactivates <strong>{deleting?.name}</strong>. If the account still has
          transactions, holdings, or investment events, the delete is rejected: clear those first.
        </ConfirmDialog>
      </>
    )
  }

  const visibleAccounts = getOrderedAccounts(accounts)
  const hiddenAccounts = getHiddenAccounts(accounts)

  // DraggableList requires items with an `id` field — use account.id (not index)
  const draggableItems = visibleAccounts.map(a => ({ ...a, id: a.id }))

  return (
    <div className="space-y-4">
      <DraggableList
        items={draggableItems}
        dragId={dragId}
        onDragChange={setDragId}
        onReorder={reorderAccounts}
        listClassName="space-y-2"
        renderItem={(a) => (
          <AccountRow
            account={a}
            profiles={profiles}
            preferredCurrency={preferredCurrency}
            fxRate={fxRateFor(a.currency)}
            leading={<DragHandle />}
            trailing={
              <>
                <Tooltip>
                  <TooltipTrigger render={<Button variant="ghost" size="icon" className="h-8 w-8 shrink-0" onClick={() => hideAccount(a.id, accounts)} />}>
                    <Eye className="h-3.5 w-3.5" />
                  </TooltipTrigger>
                  <TooltipContent>Hide from import wizard</TooltipContent>
                </Tooltip>
                <Button
                  variant="ghost"
                  size="icon"
                  className="h-8 w-8 shrink-0 opacity-0 group-hover:opacity-100"
                  onClick={() => setEditing(a)}
                  title="Edit account"
                >
                  <Pencil className="h-3.5 w-3.5" />
                </Button>
                <Button
                  variant="ghost"
                  size="icon"
                  className="h-8 w-8 shrink-0 opacity-0 group-hover:opacity-100"
                  onClick={() => requestDelete(a)}
                  title="Delete account"
                >
                  <Trash2 className="h-3.5 w-3.5" />
                </Button>
              </>
            }
          />
        )}
      />

      {hiddenAccounts.length > 0 && (
        <div>
          <p className="text-xs font-medium text-muted-foreground uppercase tracking-wider mb-2">
            Hidden from import wizard ({hiddenAccounts.length})
          </p>
          <div className="space-y-2">
            {hiddenAccounts.map(a => (
              <AccountRow
                key={a.id}
                account={a}
                profiles={profiles}
                preferredCurrency={preferredCurrency}
                fxRate={fxRateFor(a.currency)}
                showBalance={false}
                dashed
                trailing={
                  <>
                    <Tooltip>
                      <TooltipTrigger render={<Button variant="ghost" size="icon" className="h-8 w-8 shrink-0" onClick={() => showAccount(a.id, accounts)} />}>
                        <EyeOff className="h-3.5 w-3.5" />
                      </TooltipTrigger>
                      <TooltipContent>Show in import wizard</TooltipContent>
                    </Tooltip>
                    <Button variant="ghost" size="icon" className="h-8 w-8 shrink-0" onClick={() => setEditing(a)} title="Edit account">
                      <Pencil className="h-3.5 w-3.5" />
                    </Button>
                    <Button variant="ghost" size="icon" className="h-8 w-8 shrink-0" onClick={() => requestDelete(a)} title="Delete account">
                      <Trash2 className="h-3.5 w-3.5" />
                    </Button>
                  </>
                }
              />
            ))}
          </div>
        </div>
      )}

      {editing && (
        <EditAccountDialog
          account={editing}
          profiles={profiles}
          onClose={() => setEditing(null)}
          onSaved={() => { setEditing(null); onRefresh() }}
        />
      )}

      <ConfirmDialog
        open={!!deleting}
        onOpenChange={(open) => { if (!open) setDeleting(null) }}
        title="Delete account?"
        busy={deleteBusy}
        error={deleteError}
        onConfirm={handleDeleteConfirm}
      >
        This deactivates <strong>{deleting?.name}</strong>. If the account still has
        transactions, holdings, or investment events, the delete is rejected: clear those first.
      </ConfirmDialog>
    </div>
  )
}

function EditAccountDialog({ account, profiles, onClose, onSaved }: {
  account: Account
  profiles: Profile[]
  onClose: () => void
  onSaved: () => void
}) {
  const [form, setForm] = useState({
    name: account.name,
    institution: account.institution,
    type: account.type as AccountType,
    currency: account.currency,
    profileIds: account.profile_ids ?? [],
  })
  const [saving, setSaving] = useState(false)
  const [saveError, setSaveError] = useState<string | null>(null)
  const currencies = useCurrencyOptions()

  async function handleSave() {
    if (!form.name.trim() || !form.institution.trim()) return
    setSaving(true)
    setSaveError(null)
    try {
      await api.updateAccount(account.id, {
        name: form.name.trim(),
        institution: form.institution.trim(),
        type: form.type,
        currency: form.currency.toUpperCase(),
        profile_ids: form.profileIds,
      })
      onSaved()
    } catch (err) {
      setSaveError(err instanceof Error ? err.message : String(err))
    } finally {
      setSaving(false)
    }
  }

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="sm:max-w-md p-6">
        <DialogHeader><DialogTitle>Edit account</DialogTitle></DialogHeader>
        <div className="space-y-4 pt-1">
          <div>
            <label className="text-sm font-medium">Name</label>
            <Input className="mt-1.5" value={form.name} onChange={(e) => setForm((f) => ({ ...f, name: e.target.value }))} autoFocus />
          </div>
          <AccountCommonFields
            institution={form.institution}
            currency={form.currency}
            type={form.type}
            profileIds={form.profileIds}
            profiles={profiles}
            currencies={currencies}
            onChange={(patch) => setForm((f) => ({ ...f, ...patch }))}
          />
          {saveError && (
            <p className="text-xs text-destructive whitespace-pre-wrap">{saveError}</p>
          )}
          <div className="flex justify-end gap-2 pt-2">
            <Button variant="outline" size="sm" onClick={onClose}>Cancel</Button>
            <Button size="sm" onClick={handleSave} disabled={!form.name.trim() || !form.institution.trim() || saving}>
              {saving ? "Saving..." : "Save"}
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  )
}

function AccountTypeTagPicker({ value, onChange }: { value: AccountType; onChange: (t: AccountType) => void }) {
  return (
    <div className="mt-1.5 flex flex-wrap gap-2">
      {ACCOUNT_TYPE_TAGS.map((opt) => {
        if (opt.kind === "single") {
          const selected = value === opt.type
          const color = ACCOUNT_TYPE_COLORS[opt.type]
          return (
            <button
              key={opt.type}
              type="button"
              onClick={() => onChange(opt.type)}
              className={cn(
                "rounded-full border px-3 py-1 text-xs font-medium transition-all",
                selected ? "border-solid" : "border-dashed bg-transparent opacity-60 hover:opacity-100",
              )}
              style={selected ? { backgroundColor: color, borderColor: color, color: "#fff" } : { color, borderColor: color }}
            >
              {opt.label}
            </button>
          )
        }
        const selected = opt.types.includes(value)
        const color = ACCOUNT_TYPE_COLORS.investment
        const className = cn(
          "rounded-full border px-3 py-1 text-xs font-medium transition-all",
          selected ? "border-solid" : "border-dashed bg-transparent opacity-60 hover:opacity-100",
        )
        const style = selected
          ? { backgroundColor: color, borderColor: color, color: "#fff" }
          : { color, borderColor: color }
        return (
          <DropdownMenu key={opt.key}>
            <DropdownMenuTrigger className={className} style={style}>
              {opt.label}{selected ? (value === "investment_isa" ? " (ISA)" : " (non-ISA)") : ""}
            </DropdownMenuTrigger>
            <DropdownMenuContent>
              <DropdownMenuItem onClick={() => onChange("investment")}>Non-ISA</DropdownMenuItem>
              <DropdownMenuItem onClick={() => onChange("investment_isa")}>ISA</DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        )
      })}
    </div>
  )
}

function AccountTypeHelp() {
  return (
    <TooltipProvider delay={150}>
      <Tooltip>
        <TooltipTrigger className="text-muted-foreground hover:text-foreground" aria-label="About account types">
          <CircleHelp className="h-3.5 w-3.5" />
        </TooltipTrigger>
        <TooltipContent side="top" className="max-w-sm">
          <div className="space-y-2 text-left">
            <p className="text-sm font-semibold">Account types</p>
            <p className="text-muted-foreground">Sets how an account counts toward your wealth.</p>
            <ul className="space-y-1.5">
              {ACCOUNT_TYPE_HELP.map(([type, label, desc]) => (
                <li key={type} className="flex items-start gap-2">
                  <span className="mt-1 h-2 w-2 shrink-0 rounded-full" style={{ backgroundColor: ACCOUNT_TYPE_COLORS[type] }} />
                  <span>
                    <span className="font-medium">{label}</span>
                    {" "}— {desc}
                  </span>
                </li>
              ))}
            </ul>
            <p className="text-muted-foreground">
              Checking, Savings, Emergency fund &amp; Cash are <span className="font-medium text-foreground">available</span> (liquid); Pension &amp; Property are <span className="font-medium text-foreground">unavailable</span>.
            </p>
          </div>
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>
  )
}

function CurrencySelect({ value, options, onChange }: { value: string; options: string[]; onChange: (code: string) => void }) {
  const [open, setOpen] = useState(false)
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger className="w-full mt-1.5 flex h-8 items-center justify-between rounded-lg border border-input bg-transparent px-2.5 text-base md:text-sm transition-colors hover:bg-accent/50 dark:bg-input/30">
        <span>{value || "Select"}</span>
        <ChevronsUpDown className="h-3.5 w-3.5 opacity-50" />
      </PopoverTrigger>
      <PopoverContent className="w-[200px] p-0" align="start">
        <Command>
          <CommandInput placeholder="Search currency..." />
          <CommandList>
            <CommandEmpty>No currency.</CommandEmpty>
            <CommandGroup>
              {options.map((code) => (
                <CommandItem key={code} value={code} onSelect={() => { onChange(code); setOpen(false) }}>
                  <Check className={cn("mr-2 h-4 w-4", value === code ? "opacity-100" : "opacity-0")} />
                  {code}
                </CommandItem>
              ))}
            </CommandGroup>
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  )
}

function ProfileChips({ profiles, selected, onToggle }: { profiles: Profile[]; selected: string[]; onToggle: (id: string) => void }) {
  const { profileColors } = useProfileColorsContext()
  return (
    <div className="flex flex-wrap gap-1.5 mt-1.5">
      {profiles.map((p) => {
        const color = profileColors[p.id] ?? "#78716c"
        const isSel = selected.includes(p.id)
        return (
          <button
            key={p.id}
            type="button"
            onClick={() => onToggle(p.id)}
            className={cn(
              "rounded-full border px-2.5 py-1 text-xs font-medium transition-all",
              isSel ? "border-solid" : "border-dashed bg-transparent opacity-60 hover:opacity-100",
            )}
            style={isSel ? { backgroundColor: color, borderColor: color, color: "#fff" } : { color, borderColor: color }}
          >
            {p.name}
          </button>
        )
      })}
    </div>
  )
}

/** Institution + Currency, Profiles, and Type — shared by the Add and Edit dialogs. */
function AccountCommonFields({
  institution, currency, type, profileIds, profiles, currencies, onChange,
}: {
  institution: string
  currency: string
  type: AccountType
  profileIds: string[]
  profiles: Profile[]
  currencies: string[]
  onChange: (patch: Partial<{ institution: string; currency: string; type: AccountType; profileIds: string[] }>) => void
}) {
  const toggleProfile = (id: string) =>
    onChange({ profileIds: profileIds.includes(id) ? profileIds.filter((x) => x !== id) : [...profileIds, id] })
  return (
    <>
      <div className="grid grid-cols-2 gap-3">
        <div>
          <label className="text-sm font-medium">Institution</label>
          <Input className="mt-1.5" value={institution} onChange={(e) => onChange({ institution: e.target.value })} />
        </div>
        <div>
          <label className="text-sm font-medium">Currency</label>
          <CurrencySelect value={currency} options={currencies} onChange={(c) => onChange({ currency: c })} />
        </div>
      </div>
      {profiles.length > 0 && (
        <div>
          <label className="text-sm font-medium">Profiles</label>
          <ProfileChips profiles={profiles} selected={profileIds} onToggle={toggleProfile} />
        </div>
      )}
      <div>
        <div className="flex items-center gap-1.5">
          <label className="text-sm font-medium">Type</label>
          <AccountTypeHelp />
        </div>
        <AccountTypeTagPicker value={type} onChange={(t) => onChange({ type: t })} />
      </div>
    </>
  )
}

/** Currency code options for the account dialogs, from the shared currencies context. */
function useCurrencyOptions(): string[] {
  const currencies = useCurrenciesFromContext()
  return currencies.map((c) => c.code)
}

function AddAccountButton({ profiles, onRefresh }: { profiles: Profile[]; onRefresh: () => void }) {
  const [showAdd, setShowAdd] = useState(false)
  const [creating, setCreating] = useState(false)
  const [createError, setCreateError] = useState<string | null>(null)
  const [form, setForm] = useState({
    name: "", id: "", institution: "",
    type: "checking" as AccountType,
    currency: "GBP", profileIds: [] as string[], notes: "",
  })

  function slugify(text: string) {
    return text.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "")
  }

  function resetForm() {
    setForm({ name: "", id: "", institution: "", type: "checking", currency: "GBP", profileIds: [], notes: "" })
  }

  async function handleCreate() {
    if (!form.name.trim() || !form.id.trim() || !form.institution.trim()) return
    setCreating(true)
    setCreateError(null)
    try {
      await api.createAccount({
        id: form.id.trim(), name: form.name.trim(), institution: form.institution.trim(),
        type: form.type, currency: form.currency || "GBP",
        profile_ids: form.profileIds.length > 0 ? form.profileIds : undefined,
        notes: form.notes.trim() || undefined,
      })
      setShowAdd(false)
      resetForm()
      onRefresh()
    } catch (err) {
      setCreateError(err instanceof Error ? err.message : String(err))
    } finally {
      setCreating(false)
    }
  }

  const currencies = useCurrencyOptions()

  return (
    <>
      <Button size="sm" className="gap-1.5" onClick={() => { resetForm(); setCreateError(null); setShowAdd(true) }}>
        <Plus className="h-3.5 w-3.5" /> Add Account
      </Button>
      <Dialog open={showAdd} onOpenChange={setShowAdd}>
        <DialogContent className="sm:max-w-md p-6">
          <DialogHeader><DialogTitle>Add Account</DialogTitle></DialogHeader>
          <div className="space-y-4 pt-1">
            <div className="grid grid-cols-2 gap-3">
              <div>
                <label className="text-sm font-medium">Name</label>
                <Input className="mt-1.5" placeholder="e.g. Monzo Current" value={form.name}
                  onChange={(e) => setForm(f => ({ ...f, name: e.target.value, id: slugify(e.target.value) }))} autoFocus />
              </div>
              <div>
                <label className="text-sm font-medium">ID</label>
                <Input className="mt-1.5" placeholder="e.g. monzo-current" value={form.id}
                  onChange={(e) => setForm(f => ({ ...f, id: e.target.value }))} />
              </div>
            </div>
            <AccountCommonFields
              institution={form.institution}
              currency={form.currency}
              type={form.type}
              profileIds={form.profileIds}
              profiles={profiles}
              currencies={currencies}
              onChange={(patch) => setForm((f) => ({ ...f, ...patch }))}
            />
            <div>
              <label className="text-sm font-medium">Notes (optional)</label>
              <Input className="mt-1.5" placeholder="Any additional notes" value={form.notes}
                onChange={(e) => setForm(f => ({ ...f, notes: e.target.value }))} />
            </div>
            {createError && (
              <p className="text-xs text-destructive whitespace-pre-wrap">{createError}</p>
            )}
            <div className="flex justify-end gap-2">
              <Button variant="outline" size="sm" onClick={() => setShowAdd(false)}>Cancel</Button>
              <Button size="sm" onClick={handleCreate}
                disabled={!form.name.trim() || !form.id.trim() || !form.institution.trim() || creating}>
                {creating ? "Creating..." : "Create"}
              </Button>
            </div>
          </div>
        </DialogContent>
      </Dialog>
    </>
  )
}
