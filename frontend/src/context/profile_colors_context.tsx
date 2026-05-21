import { createContext, useContext, useState, useCallback } from "react"
import { COLOR_PALETTE } from "@/lib/colors"

const STORAGE_KEY = "fynance-profile-colors"

function loadStored(): Record<string, string> {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (raw) return JSON.parse(raw)
  } catch { /* ignore */ }
  return {}
}

function persist(map: Record<string, string>) {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(map))
}

function pickUnused(usedColors: string[]): string {
  const unused = COLOR_PALETTE.filter((c) => !usedColors.includes(c))
  const pool = unused.length > 0 ? unused : COLOR_PALETTE
  return pool[Math.floor(Math.random() * pool.length)]
}

interface ProfileColorsContextValue {
  /** Map of profile id → hex color. */
  profileColors: Record<string, string>
  /** Ensure every id has a color assigned. Idempotent. */
  syncProfiles: (ids: string[]) => void
  /** Override a single profile's color. Persists. */
  setColor: (id: string, color: string) => void
  /** Drop a profile's color, e.g. after deletion. */
  removeColor: (id: string) => void
}

const ProfileColorsContext = createContext<ProfileColorsContextValue | null>(null)

export function ProfileColorsProvider({ children }: { children: React.ReactNode }) {
  const [profileColors, setProfileColors] = useState<Record<string, string>>(loadStored)

  const syncProfiles = useCallback((ids: string[]) => {
    if (ids.length === 0) return
    setProfileColors((prev) => {
      let changed = false
      const usedColors = Object.values(prev)
      const next: Record<string, string> = { ...prev }
      for (const id of ids) {
        if (!next[id]) {
          next[id] = pickUnused(usedColors)
          usedColors.push(next[id])
          changed = true
        }
      }
      if (!changed) return prev
      persist(next)
      return next
    })
  }, [])

  const setColor = useCallback((id: string, color: string) => {
    setProfileColors((prev) => {
      const next = { ...prev, [id]: color }
      persist(next)
      return next
    })
  }, [])

  const removeColor = useCallback((id: string) => {
    setProfileColors((prev) => {
      if (!(id in prev)) return prev
      const next = { ...prev }
      delete next[id]
      persist(next)
      return next
    })
  }, [])

  return (
    <ProfileColorsContext.Provider value={{ profileColors, syncProfiles, setColor, removeColor }}>
      {children}
    </ProfileColorsContext.Provider>
  )
}

export function useProfileColorsContext(): ProfileColorsContextValue {
  const ctx = useContext(ProfileColorsContext)
  if (!ctx) throw new Error("useProfileColorsContext must be used inside ProfileColorsProvider")
  return ctx
}
