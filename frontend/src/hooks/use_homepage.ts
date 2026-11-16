import { useState, useCallback } from "react"

const STORAGE_KEY = "fynance-homepage"
const DEFAULT_HOME = "/portfolio"

/** One-time cleanup after the Tax tab moved under Reports: the old opt-in key and a "/tax" homepage. */
function migrateLegacyTaxTab(): void {
  localStorage.removeItem("fynance.showTaxTab")
  if (localStorage.getItem(STORAGE_KEY) === "/tax") localStorage.setItem(STORAGE_KEY, "/reports/tax")
}

export function useHomepage() {
  const [homepage, setHomepageState] = useState<string>(() => {
    try {
      migrateLegacyTaxTab()
      return localStorage.getItem(STORAGE_KEY) || DEFAULT_HOME
    } catch {
      return DEFAULT_HOME
    }
  })

  const setHomepage = useCallback((path: string) => {
    setHomepageState(path)
    localStorage.setItem(STORAGE_KEY, path)
  }, [])

  const isHomepage = useCallback(
    (path: string) => homepage === path,
    [homepage]
  )

  return { homepage, setHomepage, isHomepage }
}
