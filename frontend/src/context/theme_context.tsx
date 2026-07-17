import { createContext, useContext, useState, useEffect, useCallback } from "react"

type Theme = "light" | "dark" | "system"

const STORAGE_KEY = "fynance-theme"

function getSystemTheme(): "light" | "dark" {
  if (typeof window === "undefined") return "dark"
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light"
}

function getStoredTheme(): Theme {
  try {
    const stored = localStorage.getItem(STORAGE_KEY) as Theme | null
    if (stored === "light" || stored === "dark" || stored === "system") return stored
  } catch {
    // ignore
  }
  return "system"
}

function applyTheme(theme: Theme) {
  const resolved = theme === "system" ? getSystemTheme() : theme
  document.documentElement.classList.toggle("dark", resolved === "dark")
}

interface ThemeContextValue {
  theme: Theme
  setTheme: (t: Theme) => void
  resolvedTheme: "light" | "dark"
}

const ThemeContext = createContext<ThemeContextValue | null>(null)

/**
 * Single source of truth for the app theme. Mounted at the root so the
 * `system` mode `prefers-color-scheme` listener is always active (not just on
 * the Settings page) and live OS theme changes apply on every page.
 */
export function ThemeProvider({ children }: { children: React.ReactNode }) {
  const [theme, setThemeState] = useState<Theme>(getStoredTheme)
  // Held in state so an OS theme switch re-renders `resolvedTheme` consumers
  // (chart palettes etc.), not just the DOM class.
  const [systemTheme, setSystemTheme] = useState(getSystemTheme)

  useEffect(() => {
    applyTheme(theme)
  }, [theme])

  useEffect(() => {
    const mq = window.matchMedia("(prefers-color-scheme: dark)")
    const handler = () => {
      setSystemTheme(getSystemTheme())
      if (theme === "system") applyTheme("system")
    }
    mq.addEventListener("change", handler)
    return () => mq.removeEventListener("change", handler)
  }, [theme])

  const setTheme = useCallback((t: Theme) => {
    setThemeState(t)
    try {
      localStorage.setItem(STORAGE_KEY, t)
    } catch {
      // ignore
    }
  }, [])

  const resolvedTheme = theme === "system" ? systemTheme : theme

  return (
    <ThemeContext.Provider value={{ theme, setTheme, resolvedTheme }}>
      {children}
    </ThemeContext.Provider>
  )
}

export function useThemeContext(): ThemeContextValue {
  const ctx = useContext(ThemeContext)
  if (!ctx) throw new Error("useThemeContext must be used inside ThemeProvider")
  return ctx
}
