import { createContext, useContext } from "react"
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
  const currencies =
    currenciesData.status === "succeeded" || currenciesData.status === "reloading"
      ? currenciesData.value
      : []
  const preferredCurrency = currencies.find((c) => c.is_preferred)?.code ?? "GBP"

  return (
    <PreferredCurrencyContext.Provider value={{ preferredCurrency, currencies }}>
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
