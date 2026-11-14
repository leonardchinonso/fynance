import { useSyncExternalStore } from "react"
import { useIncomeRecords } from "@/hooks/data"

/**
 * The Tax tab is opt-in: hidden until the user either turns it on in Settings
 * or has at least one income record. The flag lives at module scope
 * (mirrored to localStorage) so the Settings switch updates the navbar in place.
 */
const SHOW_TAX_TAB_KEY = "fynance.showTaxTab"

let _showTaxTab = (() => {
  try {
    return localStorage.getItem(SHOW_TAX_TAB_KEY) === "1"
  } catch {
    return false
  }
})()

const listeners = new Set<() => void>()

function subscribe(cb: () => void): () => void {
  listeners.add(cb)
  return () => {
    listeners.delete(cb)
  }
}

function getShowTaxTab(): boolean {
  return _showTaxTab
}

export function setShowTaxTab(value: boolean): void {
  _showTaxTab = value
  try {
    if (value) localStorage.setItem(SHOW_TAX_TAB_KEY, "1")
    else localStorage.removeItem(SHOW_TAX_TAB_KEY)
  } catch {
    /* ignore: the choice still applies for this session */
  }
  for (const cb of listeners) cb()
}

/** The manual "Show Tax tab" setting. */
export function useShowTaxTab(): boolean {
  return useSyncExternalStore(subscribe, getShowTaxTab)
}

/**
 * Whether the navbar shows the Tax tab: the manual setting, or any
 * income record in any profile. A failed request
 * (e.g. an older backend without the endpoint) just keeps the tab hidden.
 */
export function useTaxTabVisible(): boolean {
  const manualOn = useShowTaxTab()
  const [rows] = useIncomeRecords({ enabled: !manualOn })
  if (manualOn) return true
  return (rows.status === "succeeded" || rows.status === "reloading") && rows.value.length > 0
}
