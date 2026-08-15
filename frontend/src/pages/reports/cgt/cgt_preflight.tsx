import { useMemo, useState } from "react"
import { AlertTriangle, Check, Info } from "lucide-react"
import { api } from "@/api/client"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import type { MissingRatePair } from "@/bindings/MissingRatePair"
import type { Profile } from "@/types"

/**
 * The pre-flight review step that precedes report generation.
 *
 * Rather than the report failing with an error the user has to decode, everything needing
 * confirmation is surfaced in one place and resolved before generation is offered. A missing
 * exchange rate is therefore neither an error nor a warning — it is a pre-flight item.
 *
 * The rates are USER-OWNED and that is the whole design, not an implementation detail: HMRC
 * mandates no particular rate source, only that the chosen basis is applied consistently, and the
 * main use case here is reproducing the rates a previously-filed return was computed with.
 * Nothing on this screen invents a value; an auto-fill button (not built yet) would only
 * pre-populate a field the user still commits.
 */

/** Rates keyed by `${currency}|${date}`, holding the raw text the user typed. */
type RateDrafts = Record<string, string>

function keyOf(pair: MissingRatePair): string {
  return `${pair.currency}|${pair.date}`
}

/**
 * A rate must be a positive decimal. Parsed with a strict regex rather than `Number` because
 * `Number("")` is 0 and `Number("1,5")` is NaN in a way that is easy to mis-handle — and a
 * silently-wrong rate is exactly the failure this feature exists to prevent.
 */
function isValidRate(raw: string): boolean {
  const t = raw.trim()
  if (!/^\d*\.?\d+$/.test(t)) return false
  return Number.parseFloat(t) > 0
}

export function CgtPreflight({
  missing,
  quote,
  profile,
  onReady,
  onCancel,
}: {
  missing: MissingRatePair[]
  /** Currency every rate is quoted into — the preferred currency. */
  quote: string
  /** The profile the report is being generated for; supplies the UTR to confirm. */
  profile: Profile | undefined
  /**
   * Called once every rate is saved. Receives the UTR the user confirmed, which the caller
   * snapshots onto the generated report.
   */
  onReady: (confirmedUtr: string | null) => void
  onCancel: () => void
}) {
  const [drafts, setDrafts] = useState<RateDrafts>({})
  const [utr, setUtr] = useState(profile?.utr ?? "")
  const [saving, setSaving] = useState(false)
  const [saveError, setSaveError] = useState<string | null>(null)

  // Group by currency. A tax year commonly needs ~49 rates across only one or two currencies,
  // so grouping turns a wall of rows into a couple of short, scannable date lists.
  const byCurrency = useMemo(() => {
    const groups = new Map<string, MissingRatePair[]>()
    for (const pair of missing) {
      const list = groups.get(pair.currency) ?? []
      list.push(pair)
      groups.set(pair.currency, list)
    }
    for (const list of groups.values()) list.sort((a, b) => a.date.localeCompare(b.date))
    return [...groups.entries()].sort(([a], [b]) => a.localeCompare(b))
  }, [missing])

  const filledCount = missing.filter((p) => isValidRate(drafts[keyOf(p)] ?? "")).length
  const invalidCount = missing.filter((p) => {
    const raw = drafts[keyOf(p)] ?? ""
    return raw.trim() !== "" && !isValidRate(raw)
  }).length
  const allFilled = filledCount === missing.length && missing.length > 0

  const trimmedUtr = utr.replace(/\s/g, "")
  const utrValid = trimmedUtr === "" || /^\d{10}$/.test(trimmedUtr)

  /**
   * Apply one rate to every outstanding date for a currency.
   *
   * A convenience only — it fills the inputs, it does not submit them, so the user still sees
   * and commits every value. Genuinely useful when reproducing a filed return that used a single
   * average rate for the year, which HMRC permits.
   */
  function applyToAll(currency: string, value: string) {
    setDrafts((prev) => {
      const next = { ...prev }
      for (const pair of missing) {
        if (pair.currency === currency) next[keyOf(pair)] = value
      }
      return next
    })
  }

  async function handleSave() {
    if (!allFilled || !utrValid) return
    setSaving(true)
    setSaveError(null)
    try {
      // Persist the UTR only when the user actually changed it, so opening the screen and
      // generating does not rewrite a profile field that was already correct.
      const currentUtr = profile?.utr ?? ""
      if (profile && trimmedUtr !== currentUtr) {
        await api.updateProfile(profile.id, { utr: trimmedUtr === "" ? null : trimmedUtr })
      }

      await api.createExchangeRates(
        missing.map((pair) => ({
          base: pair.currency,
          quote,
          date: pair.date,
          rate: drafts[keyOf(pair)].trim(),
          // Typed in by hand. A provider-suggested value that the user accepted would be
          // recorded as "suggested" so the report can show where each rate came from.
          source: "user",
        })),
      )
      onReady(trimmedUtr === "" ? null : trimmedUtr)
    } catch (err) {
      setSaveError(err instanceof Error ? err.message : String(err))
    } finally {
      setSaving(false)
    }
  }

  return (
    <Card className="border-amber-400/50">
      <CardHeader>
        <div className="flex items-start gap-3">
          <AlertTriangle className="mt-0.5 h-5 w-5 shrink-0 text-amber-600 dark:text-amber-400" />
          <div>
            <CardTitle>Before generating this report</CardTitle>
            <p className="mt-1 text-sm text-muted-foreground">
              Confirm the details below. They are saved and you will not be asked again.
            </p>
          </div>
        </div>
      </CardHeader>
      <CardContent className="space-y-6">
        {/* ── Exchange rates ─────────────────────────────────────────────── */}
        <section className="space-y-3">
          <div className="flex items-baseline justify-between gap-3">
            <h3 className="text-sm font-semibold">
              Exchange rates ({filledCount} of {missing.length})
            </h3>
            {allFilled && (
              <span className="flex items-center gap-1 text-xs text-emerald-600 dark:text-emerald-400">
                <Check className="h-3.5 w-3.5" /> All set
              </span>
            )}
          </div>

          <div className="flex items-start gap-2 rounded-md bg-muted/50 p-3">
            <Info className="mt-0.5 h-4 w-4 shrink-0 text-muted-foreground" />
            <p className="text-xs text-muted-foreground">
              HMRC requires each disposal converted at its own date&apos;s rate, and each
              acquisition at the rate on the date it was acquired — so rates are needed for every
              acquisition still in the pool, <strong>including ones from earlier tax years</strong>.
              Enter the number of <strong>{quote}</strong> per 1 unit of the foreign currency (for
              example 0.7862 means 1 USD = 0.7862 {quote}). Any consistently-applied basis is
              acceptable; these are your figures, not fetched ones.
            </p>
          </div>

          {byCurrency.map(([currency, pairs]) => (
            <div key={currency} className="rounded-lg border">
              <div className="flex flex-wrap items-center justify-between gap-2 border-b bg-muted/30 px-3 py-2">
                <p className="text-sm font-medium">
                  {currency} → {quote}{" "}
                  <span className="text-xs font-normal text-muted-foreground">
                    ({pairs.length} {pairs.length === 1 ? "date" : "dates"})
                  </span>
                </p>
                <ApplyToAll currency={currency} onApply={applyToAll} />
              </div>
              <div className="divide-y">
                {pairs.map((pair) => {
                  const k = keyOf(pair)
                  const raw = drafts[k] ?? ""
                  const bad = raw.trim() !== "" && !isValidRate(raw)
                  return (
                    <div key={k} className="flex items-center gap-3 px-3 py-2">
                      <label htmlFor={k} className="w-28 shrink-0 font-mono text-xs">
                        {pair.date}
                      </label>
                      <Input
                        id={k}
                        value={raw}
                        onChange={(e) =>
                          setDrafts((prev) => ({ ...prev, [k]: e.target.value }))
                        }
                        placeholder="0.7862"
                        inputMode="decimal"
                        aria-invalid={bad}
                        className={`h-8 max-w-40 ${bad ? "border-destructive" : ""}`}
                      />
                      <span className="text-xs text-muted-foreground">
                        {quote} per 1 {currency}
                      </span>
                    </div>
                  )
                })}
              </div>
            </div>
          ))}

          {invalidCount > 0 && (
            <p className="text-xs text-destructive">
              {invalidCount} {invalidCount === 1 ? "rate is" : "rates are"} not a positive number.
            </p>
          )}
        </section>

        {/* ── UTR ────────────────────────────────────────────────────────── */}
        <section className="space-y-2">
          <h3 className="text-sm font-semibold">Unique Taxpayer Reference</h3>
          <Input
            value={utr}
            onChange={(e) => setUtr(e.target.value)}
            placeholder="10 digits"
            inputMode="numeric"
            aria-invalid={!utrValid}
            className={`max-w-56 ${utrValid ? "" : "border-destructive"}`}
          />
          <p className="text-xs text-muted-foreground">
            Printed on the generated report. Saved to{" "}
            <strong>{profile?.name ?? "this profile"}</strong>. Leave blank to omit it.
          </p>
          {!utrValid && <p className="text-xs text-destructive">A UTR is exactly 10 digits.</p>}
        </section>

        {/* ── Not yet implemented ────────────────────────────────────────── */}
        <section className="space-y-2">
          <h3 className="text-sm font-semibold text-muted-foreground">
            Losses and allowances
          </h3>
          <p className="text-xs text-muted-foreground">
            Brought-forward losses and the Annual Exempt Amount are not editable here yet. The
            report currently applies the statutory allowance for the tax year and assumes no
            brought-forward losses. If either applies to you, check the figures before filing.
          </p>
        </section>

        {saveError && (
          <p className="text-sm text-destructive whitespace-pre-wrap">{saveError}</p>
        )}

        <div className="flex justify-end gap-2">
          <Button variant="outline" size="sm" onClick={onCancel} disabled={saving}>
            Cancel
          </Button>
          <Button
            size="sm"
            onClick={handleSave}
            disabled={!allFilled || !utrValid || saving}
          >
            {saving ? "Saving…" : "Save and generate"}
          </Button>
        </div>
      </CardContent>
    </Card>
  )
}

/** Fills every outstanding date for one currency with the same value. */
function ApplyToAll({
  currency,
  onApply,
}: {
  currency: string
  onApply: (currency: string, value: string) => void
}) {
  const [value, setValue] = useState("")
  return (
    <div className="flex items-center gap-2">
      <Input
        value={value}
        onChange={(e) => setValue(e.target.value)}
        placeholder="Same rate for all"
        inputMode="decimal"
        className="h-7 max-w-40 text-xs"
      />
      <Button
        variant="outline"
        size="sm"
        className="h-7"
        disabled={!isValidRate(value)}
        onClick={() => onApply(currency, value.trim())}
      >
        Apply to all
      </Button>
    </div>
  )
}
