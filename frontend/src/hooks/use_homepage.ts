import { useState, useCallback } from "react"

const STORAGE_KEY = "fynance-homepage"
const DEFAULT_HOME = "/portfolio"

export function useHomepage() {
  const [homepage, setHomepageState] = useState<string>(() => {
    try {
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
