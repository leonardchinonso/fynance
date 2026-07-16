import { createContext, useContext, useMemo, useRef } from "react"
import type { Currency } from "@/types"
import { useCurrencies } from "@/hooks/data/use_currencies"

interface PreferredCurrencyContextValue {
  preferredCurrency: string
  currencies: Currency[]
}

const PreferredCurrencyContext = createContext<PreferredCurrencyContextValue | null>(null)

/**
 * Preferred currency + FX rates, derived from the shared cached currencies
 * query (same cache entry as {@link useCurrencies}). Currency mutations
 * invalidate the whole cache via the api client, so this stays fresh without
 * manual wiring.
 */
export function PreferredCurrencyProvider({ children }: { children: React.ReactNode }) {
  const [currenciesData] = useCurrencies()
  const fresh =
    currenciesData.status === "succeeded" || currenciesData.status === "reloading"
      ? currenciesData.value
      : null
  // Keep the last good rates through a failed refetch: a backend blip must not
  // re-base every money label to GBP or wipe FX conversions.
  const lastGood = useRef<Currency[]>([])
  if (fresh) lastGood.current = fresh
  const currencies = fresh ?? lastGood.current
  const preferredCurrency = currencies.find((c) => c.is_preferred)?.code ?? "GBP"

  // Stable value identity: consumers re-render only when the rates change,
  // not on every stale-while-revalidate emit.
  const value = useMemo(() => ({ preferredCurrency, currencies }), [preferredCurrency, currencies])

  return (
    <PreferredCurrencyContext.Provider value={value}>
      {children}
    </PreferredCurrencyContext.Provider>
  )
}

export function usePreferredCurrency(): string {
  const ctx = useContext(PreferredCurrencyContext)
  if (!ctx) throw new Error("usePreferredCurrency must be used inside PreferredCurrencyProvider")
  return ctx.preferredCurrency
}

export function useCurrenciesFromContext(): Currency[] {
  const ctx = useContext(PreferredCurrencyContext)
  if (!ctx) throw new Error("useCurrenciesFromContext must be used inside PreferredCurrencyProvider")
  return ctx.currencies
}
