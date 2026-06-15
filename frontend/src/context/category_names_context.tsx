import { createContext, useContext, useState, useEffect, useCallback } from "react"
import { api } from "@/api/client"
import type { CategoryNode } from "@/bindings/CategoryNode"

/**
 * Resolves a `category_id` to its display name ("Parent: Child").
 *
 * The API returns `category_id` only (the normalized FK); the human-readable
 * name lives in the categories table, which this context loads once. Unknown
 * ids fall through to the id verbatim — which is also how mock mode works, where
 * `category_id` carries the display-name string directly.
 */
interface CategoryNamesContextValue {
  resolve: (id: string | null | undefined) => string
  refresh: () => void
}

const CategoryNamesContext = createContext<CategoryNamesContextValue | null>(null)

function buildNameMap(tree: CategoryNode[]): Map<string, string> {
  const map = new Map<string, string>()
  for (const parent of tree) {
    map.set(parent.id, parent.name)
    for (const child of parent.children) {
      map.set(child.id, `${parent.name}: ${child.name}`)
    }
  }
  return map
}

export function CategoryNamesProvider({ children }: { children: React.ReactNode }) {
  const [nameMap, setNameMap] = useState<Map<string, string>>(new Map())

  const load = useCallback(() => {
    api
      .getCategoryDetails()
      .then((tree) => setNameMap(buildNameMap(tree)))
      .catch(() => {})
  }, [])

  useEffect(() => {
    load()
  }, [load])

  const resolve = useCallback(
    (id: string | null | undefined): string => {
      if (!id) return "Uncategorized"
      return nameMap.get(id) ?? id
    },
    [nameMap],
  )

  return (
    <CategoryNamesContext.Provider value={{ resolve, refresh: load }}>
      {children}
    </CategoryNamesContext.Provider>
  )
}

export function useResolveCategoryName(): (id: string | null | undefined) => string {
  const ctx = useContext(CategoryNamesContext)
  if (!ctx) throw new Error("useResolveCategoryName must be used inside CategoryNamesProvider")
  return ctx.resolve
}

export function useRefreshCategoryNames(): () => void {
  const ctx = useContext(CategoryNamesContext)
  if (!ctx) throw new Error("useRefreshCategoryNames must be used inside CategoryNamesProvider")
  return ctx.refresh
}
