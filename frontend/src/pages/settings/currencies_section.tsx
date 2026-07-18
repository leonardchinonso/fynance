import { useState } from "react"
import { api } from "@/api/client"
import type { Currency } from "@/types"
import type { RemoteData } from "@/lib/remote_data"
import { visitRemoteData } from "@/lib/remote_data"
import { SettingsListSkeleton } from "@/components/skeletons"
import { AuthAwareError } from "@/components/auth_aware_error"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem } from "@/components/ui/command"
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover"
import { Star, Plus, Trash2, Check, ChevronsUpDown } from "lucide-react"
import { cn, daysSince } from "@/lib/utils"

const ISO_CURRENCY_NAMES: Record<string, string> = {
  GBP: "British Pound",
  GBX: "British Pence",
  USD: "US Dollar",
  EUR: "Euro",
  NGN: "Nigerian Naira",
  JPY: "Japanese Yen",
  CAD: "Canadian Dollar",
  AUD: "Australian Dollar",
  CHF: "Swiss Franc",
  CNY: "Chinese Yuan",
  INR: "Indian Rupee",
  BRL: "Brazilian Real",
  MXN: "Mexican Peso",
  ZAR: "South African Rand",
  ZAC: "South African Cent",
  KES: "Kenyan Shilling",
  GHS: "Ghanaian Cedi",
  EGP: "Egyptian Pound",
  MAD: "Moroccan Dirham",
}

/** Long-form hints rendered as muted text alongside the title. Used for
 * non-ISO sub-unit codes where the rate-to-parent isn't obvious. */
const CURRENCY_NOTES: Record<string, string> = {
  GBX: "LSE sub-unit, 1 GBX = 0.01 GBP",
  ZAC: "Sub-unit, 1 ZAC = 0.01 ZAR",
}

const COMMON_CURRENCY_CODES = Object.keys(ISO_CURRENCY_NAMES)

export function CurrenciesSection({
  data,
  onRefresh,
}: {
  data: RemoteData<Currency[]>
  onRefresh: () => void
}) {
  return visitRemoteData(data, {
    notLoaded: () => (
      <CurrenciesCard loading onRefresh={onRefresh}>
        <SettingsListSkeleton rows={2} />
      </CurrenciesCard>
    ),
    failed: (error) => (
      <CurrenciesCard onRefresh={onRefresh}>
        <AuthAwareError error={error} onRetry={onRefresh} />
      </CurrenciesCard>
    ),
    hasValue: (currencies) => (
      <CurrenciesCard onRefresh={onRefresh}>
        <CurrenciesList currencies={currencies} onRefresh={onRefresh} />
      </CurrenciesCard>
    ),
  })
}

function CurrenciesCard({
  children,
  loading,
  onRefresh,
}: {
  children: React.ReactNode
  loading?: boolean
  onRefresh: () => void
}) {
  return (
    <Card id="currencies">
      <CardHeader>
        <div className="flex items-center justify-between">
          <CardTitle className="text-lg">Currencies</CardTitle>
          {!loading && <AddCurrencyButton onRefresh={onRefresh} />}
        </div>
        <p className="text-sm text-muted-foreground">
          Your preferred currency is used for all portfolio calculations. Other currencies are converted using the exchange rates you provide.
        </p>
      </CardHeader>
      <CardContent>{children}</CardContent>
    </Card>
  )
}

function CurrenciesList({ currencies, onRefresh }: { currencies: Currency[]; onRefresh: () => void }) {
  const preferred = currencies.find((c) => c.is_preferred)

  if (currencies.length === 0) {
    return (
      <p className="text-sm text-muted-foreground py-4 text-center">
        No currencies configured. Add one to get started.
      </p>
    )
  }

  return (
    <div className="space-y-2">
      {currencies.map((c) => (
        <CurrencyRow
          key={c.code}
          currency={c}
          preferredCode={preferred?.code ?? "GBP"}
          onRefresh={onRefresh}
        />
      ))}
    </div>
  )
}

function CurrencyRow({
  currency,
  preferredCode,
  onRefresh,
}: {
  currency: Currency
  preferredCode: string
  onRefresh: () => void
}) {
  const [rate, setRate] = useState(currency.fx_rate)
  const [rateDirty, setRateDirty] = useState(false)
  const [saving, setSaving] = useState(false)
  const [confirmPreferred, setConfirmPreferred] = useState(false)

  const name = ISO_CURRENCY_NAMES[currency.code] ?? currency.code
  const staleDays = currency.updated_at ? daysSince(currency.updated_at) : null
  const isStale = staleDays !== null && staleDays > 30

  async function handleRateSave() {
    setSaving(true)
    try {
      await api.updateCurrency(currency.code, { fx_rate: rate })
      setRateDirty(false)
      onRefresh()
    } finally {
      setSaving(false)
    }
  }

  async function handleSetPreferred() {
    setConfirmPreferred(false)
    await api.updateCurrency(currency.code, { is_preferred: true })
    onRefresh()
  }

  async function handleDelete() {
    await api.deleteCurrency(currency.code)
    onRefresh()
  }

  return (
    <>
      <div className="flex items-center gap-3 rounded-lg border p-3">
        <Tooltip>
          <TooltipTrigger
            className="shrink-0 text-muted-foreground hover:text-yellow-500 transition-colors disabled:opacity-40"
            onClick={() => !currency.is_preferred && setConfirmPreferred(true)}
            disabled={currency.is_preferred}
          >
            <Star
              className={cn("h-4 w-4", currency.is_preferred && "fill-yellow-400 text-yellow-400")}
            />
          </TooltipTrigger>
          <TooltipContent>
            {currency.is_preferred ? "Preferred currency" : "Set as preferred"}
          </TooltipContent>
        </Tooltip>

        {/* Identity + rate editor: stacked below sm so the code/name and the
            rate input don't collide; inline from sm up. */}
        <div className="flex-auto min-w-0 flex flex-col gap-2 sm:flex-row sm:items-center sm:gap-3">
          <div className="flex items-center gap-2 min-w-0 sm:flex-auto">
            <span className="font-mono font-semibold shrink-0">{currency.code}</span>
            <span className="text-sm text-muted-foreground truncate">{name}</span>
            {isStale && (
              <span className="text-xs text-amber-500 shrink-0">{staleDays}d old</span>
            )}
          </div>

          {!currency.is_preferred && (
            <div className="flex items-center gap-2 shrink-0">
              <span className="text-xs text-muted-foreground whitespace-nowrap">
                1 {currency.code} =
              </span>
              <Input
                className="w-24 h-7 text-sm"
                value={rate}
                onChange={(e) => { setRate(e.target.value); setRateDirty(true) }}
              />
              <span className="text-xs text-muted-foreground">{preferredCode}</span>
              {rateDirty && (
                <Button size="sm" className="h-7 text-xs" onClick={handleRateSave} disabled={saving}>
                  Save
                </Button>
              )}
            </div>
          )}

          {currency.is_preferred && (
            <span className="text-xs text-muted-foreground shrink-0">Preferred</span>
          )}
        </div>

        <Tooltip>
          <TooltipTrigger
            className="inline-flex h-7 w-7 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:text-red-500 hover:bg-accent transition-colors disabled:opacity-40"
            onClick={handleDelete}
            disabled={currency.is_preferred}
          >
            <Trash2 className="h-3.5 w-3.5" />
          </TooltipTrigger>
          <TooltipContent>
            {currency.is_preferred ? "Cannot delete the preferred currency" : "Remove currency"}
          </TooltipContent>
        </Tooltip>
      </div>

      <Dialog open={confirmPreferred} onOpenChange={setConfirmPreferred}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Change preferred currency?</DialogTitle>
          </DialogHeader>
          <p className="text-sm text-muted-foreground">
            Changing your preferred currency to {currency.code} will require you to re-enter exchange rates for all other currencies.
          </p>
          <div className="flex justify-end gap-2 mt-4">
            <Button variant="outline" onClick={() => setConfirmPreferred(false)}>Cancel</Button>
            <Button onClick={handleSetPreferred}>Confirm</Button>
          </div>
        </DialogContent>
      </Dialog>
    </>
  )
}

function AddCurrencyButton({ onRefresh }: { onRefresh: () => void }) {
  const [open, setOpen] = useState(false)
  return (
    <>
      <Button size="sm" variant="outline" onClick={() => setOpen(true)}>
        <Plus className="h-4 w-4 mr-1" />
        Add
      </Button>
      <AddCurrencyDialog open={open} onOpenChange={setOpen} onRefresh={onRefresh} />
    </>
  )
}

function AddCurrencyDialog({
  open,
  onOpenChange,
  onRefresh,
}: {
  open: boolean
  onOpenChange: (v: boolean) => void
  onRefresh: () => void
}) {
  const [code, setCode] = useState("")
  const [rate, setRate] = useState("")
  const [comboOpen, setComboOpen] = useState(false)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  async function handleSave() {
    if (!code || !rate) return
    setSaving(true)
    setError(null)
    try {
      await api.createCurrency({ code, fx_rate: rate })
      setCode("")
      setRate("")
      onOpenChange(false)
      onRefresh()
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to add currency")
    } finally {
      setSaving(false)
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Add Currency</DialogTitle>
        </DialogHeader>
        <div className="space-y-4">
          <div className="space-y-1.5">
            <label className="text-sm font-medium">Currency</label>
            <Popover open={comboOpen} onOpenChange={setComboOpen}>
              <PopoverTrigger
                role="combobox"
                className="flex h-9 w-full items-center justify-between rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background placeholder:text-muted-foreground focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2"
              >
                {code
                  ? `${code} — ${ISO_CURRENCY_NAMES[code] ?? code}`
                  : "Select currency…"}
                <ChevronsUpDown className="h-4 w-4 opacity-50 ml-2 shrink-0" />
              </PopoverTrigger>
              <PopoverContent className="w-full p-0">
                <Command>
                  <CommandInput placeholder="Search currency…" />
                  <CommandEmpty>No currency found.</CommandEmpty>
                  <CommandGroup>
                    {COMMON_CURRENCY_CODES.map((c) => (
                      <CommandItem
                        key={c}
                        value={c}
                        onSelect={(val) => {
                          setCode(val.toUpperCase())
                          setComboOpen(false)
                        }}
                      >
                        <Check className={cn("mr-2 h-4 w-4", code === c ? "opacity-100" : "opacity-0")} />
                        <div className="flex min-w-0 flex-col">
                          <span>{c} — {ISO_CURRENCY_NAMES[c]}</span>
                          {CURRENCY_NOTES[c] && (
                            <span className="text-xs text-muted-foreground">
                              {CURRENCY_NOTES[c]}
                            </span>
                          )}
                        </div>
                      </CommandItem>
                    ))}
                  </CommandGroup>
                </Command>
              </PopoverContent>
            </Popover>
          </div>

          <div className="space-y-1.5">
            <label className="text-sm font-medium">Exchange rate</label>
            <div className="flex items-center gap-2">
              <span className="text-sm text-muted-foreground whitespace-nowrap">1 {code || "CCY"} =</span>
              <Input
                placeholder="0.00"
                value={rate}
                onChange={(e) => setRate(e.target.value)}
                type="number"
                step="any"
              />
            </div>
          </div>

          {error && <p className="text-sm text-red-500">{error}</p>}

          <div className="flex justify-end gap-2">
            <Button variant="outline" onClick={() => onOpenChange(false)}>Cancel</Button>
            <Button onClick={handleSave} disabled={!code || !rate || saving}>
              Add Currency
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  )
}
