import { createContext, useContext, useState, useEffect, useCallback } from "react"
import { api } from "@/api/client"

interface PreferredCurrencyContextValue {
  preferredCurrency: string
  refreshPreferredCurrency: () => void
}

const PreferredCurrencyContext = createContext<PreferredCurrencyContextValue | null>(null)

export function PreferredCurrencyProvider({ children }: { children: React.ReactNode }) {
  const [preferredCurrency, setPreferredCurrency] = useState("GBP")

  const load = useCallback(() => {
    api.getCurrencies().then((currencies) => {
      const preferred = currencies.find((c) => c.is_preferred)
      if (preferred) setPreferredCurrency(preferred.code)
    }).catch(() => {})
  }, [])

  useEffect(() => { load() }, [load])

  return (
    <PreferredCurrencyContext.Provider value={{ preferredCurrency, refreshPreferredCurrency: load }}>
      {children}
    </PreferredCurrencyContext.Provider>
  )
}

export function usePreferredCurrency(): string {
  const ctx = useContext(PreferredCurrencyContext)
  if (!ctx) throw new Error("usePreferredCurrency must be used inside PreferredCurrencyProvider")
  return ctx.preferredCurrency
}

export function useRefreshPreferredCurrency(): () => void {
  const ctx = useContext(PreferredCurrencyContext)
  if (!ctx) throw new Error("useRefreshPreferredCurrency must be used inside PreferredCurrencyProvider")
  return ctx.refreshPreferredCurrency
}
