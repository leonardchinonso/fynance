import { createContext, useContext, useState, useCallback } from "react"
import { COLOR_PALETTE, CATEGORY_COLORS } from "@/lib/colors"

const STORAGE_KEY = "fynance-category-colors"

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
  const unused = COLOR_PALETTE.filter(c => !usedColors.includes(c))
  const pool = unused.length > 0 ? unused : COLOR_PALETTE
  return pool[Math.floor(Math.random() * pool.length)]
}

interface CategoryColorsContextValue {
  categoryColors: Record<string, string>
  syncParents: (parentNames: string[]) => void
  setColor: (name: string, color: string) => void
}

const CategoryColorsContext = createContext<CategoryColorsContextValue | null>(null)

export function CategoryColorsProvider({ children }: { children: React.ReactNode }) {
  const [categoryColors, setCategoryColors] = useState<Record<string, string>>(loadStored)

  const syncParents = useCallback((parentNames: string[]) => {
    if (parentNames.length === 0) return
    setCategoryColors(prev => {
      const usedColors = Object.values(prev)
      const next: Record<string, string> = {}

      for (const name of parentNames) {
        if (prev[name]) {
          next[name] = prev[name]
        } else if (CATEGORY_COLORS[name]) {
          next[name] = CATEGORY_COLORS[name]
        } else {
          next[name] = pickUnused(usedColors)
          usedColors.push(next[name])
        }
      }

      persist(next)
      return next
    })
  }, [])

  const setColor = useCallback((name: string, color: string) => {
    setCategoryColors(prev => {
      const next = { ...prev, [name]: color }
      persist(next)
      return next
    })
  }, [])

  return (
    <CategoryColorsContext.Provider value={{ categoryColors, syncParents, setColor }}>
      {children}
    </CategoryColorsContext.Provider>
  )
}

export function useCategoryColorsContext(): CategoryColorsContextValue {
  const ctx = useContext(CategoryColorsContext)
  if (!ctx) throw new Error("useCategoryColorsContext must be used inside CategoryColorsProvider")
  return ctx
}
