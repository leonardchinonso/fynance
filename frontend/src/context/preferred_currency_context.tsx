import { createContext, useContext, useState, useEffect, useCallback } from "react"
import { api } from "@/api/client"
import type { Currency } from "@/types"

interface PreferredCurrencyContextValue {
  preferredCurrency: string
  currencies: Currency[]
  refreshPreferredCurrency: () => void
}

const PreferredCurrencyContext = createContext<PreferredCurrencyContextValue | null>(null)

export function PreferredCurrencyProvider({ children }: { children: React.ReactNode }) {
  const [preferredCurrency, setPreferredCurrency] = useState("GBP")
  const [currencies, setCurrencies] = useState<Currency[]>([])

  const load = useCallback(() => {
    api.getCurrencies().then((result) => {
      setCurrencies(result)
      const preferred = result.find((c) => c.is_preferred)
      if (preferred) setPreferredCurrency(preferred.code)
    }).catch(() => {})
  }, [])

  useEffect(() => { load() }, [load])

  return (
    <PreferredCurrencyContext.Provider value={{ preferredCurrency, currencies, refreshPreferredCurrency: load }}>
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

export function useRefreshPreferredCurrency(): () => void {
  const ctx = useContext(PreferredCurrencyContext)
  if (!ctx) throw new Error("useRefreshPreferredCurrency must be used inside PreferredCurrencyProvider")
  return ctx.refreshPreferredCurrency
}
