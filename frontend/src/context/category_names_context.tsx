import { createContext, useContext, useMemo, useCallback, useRef } from "react"
import type { CategoryNode } from "@/bindings/CategoryNode"
import type { CategoryType } from "@/bindings/CategoryType"
import { useCategories } from "@/hooks/data/use_categories"

/**
 * Resolves a `category_id` to its display name ("Parent: Child"), its
 * `category_type`, and parent grouping/ordering info.
 *
 * The API returns ids only; the human-readable name, type and hierarchy live in
 * the categories table. The maps derive from the shared cached category query
 * (same cache entry as {@link useCategories}), so category mutations, which
 * invalidate the whole cache via the api client, refresh names everywhere
 * without a page reload. Unknown ids fall through to the id verbatim, which is
 * also how mock mode works, where `category_id` carries the display-name string
 * directly.
 */
interface CategoryMaps {
  /** leaf/parent id -> "Parent: Child" (or "Parent" for a parent id) */
  nameMap: Map<string, string>
  /** leaf/parent id -> category_type */
  typeMap: Map<string, CategoryType>
  /** parent id -> parent name */
  parentNameMap: Map<string, string>
  /** parent id -> its leaf child ids (for drilling a parent into its leaves) */
  childIdsMap: Map<string, string[]>
  /** parent ids in display order (drives spreadsheet group order) */
  parentOrder: string[]
}

interface CategoryNamesContextValue {
  resolve: (id: string | null | undefined) => string
  categoryType: (id: string | null | undefined) => CategoryType | undefined
  parentName: (parentId: string | null | undefined) => string
  /** Leaf child ids under a parent id (empty if unknown). */
  childIdsOf: (parentId: string | null | undefined) => string[]
  parentOrder: string[]
}

const CategoryNamesContext = createContext<CategoryNamesContextValue | null>(null)

function buildMaps(tree: CategoryNode[]): CategoryMaps {
  const nameMap = new Map<string, string>()
  const typeMap = new Map<string, CategoryType>()
  const parentNameMap = new Map<string, string>()
  const childIdsMap = new Map<string, string[]>()
  const parentOrder: string[] = []
  for (const parent of tree) {
    nameMap.set(parent.id, parent.name)
    typeMap.set(parent.id, parent.category_type)
    parentNameMap.set(parent.id, parent.name)
    childIdsMap.set(parent.id, parent.children.map((c) => c.id))
    parentOrder.push(parent.id)
    for (const child of parent.children) {
      nameMap.set(child.id, `${parent.name}: ${child.name}`)
      typeMap.set(child.id, child.category_type)
    }
  }
  return { nameMap, typeMap, parentNameMap, childIdsMap, parentOrder }
}

const EMPTY_MAPS: CategoryMaps = {
  nameMap: new Map(),
  typeMap: new Map(),
  parentNameMap: new Map(),
  childIdsMap: new Map(),
  parentOrder: [],
}

export function CategoryNamesProvider({ children }: { children: React.ReactNode }) {
  const [categoriesData] = useCategories()
  const fresh =
    categoriesData.status === "succeeded" || categoriesData.status === "reloading"
      ? categoriesData.value
      : null
  // Keep the last good tree through a failed refetch (e.g. a forced
  // invalidation while the backend restarts): stale names beat raw ids.
  const lastGood = useRef<CategoryNode[] | null>(null)
  if (fresh) lastGood.current = fresh
  const tree = fresh ?? lastGood.current
  const maps = useMemo(() => (tree ? buildMaps(tree) : EMPTY_MAPS), [tree])

  const resolve = useCallback(
    (id: string | null | undefined): string => {
      if (!id) return "Uncategorized"
      return maps.nameMap.get(id) ?? id
    },
    [maps],
  )

  const categoryType = useCallback(
    (id: string | null | undefined): CategoryType | undefined => {
      if (!id) return undefined
      return maps.typeMap.get(id)
    },
    [maps],
  )

  const parentName = useCallback(
    (parentId: string | null | undefined): string => {
      if (!parentId) return "Uncategorized"
      return maps.parentNameMap.get(parentId) ?? parentId
    },
    [maps],
  )

  const childIdsOf = useCallback(
    (parentId: string | null | undefined): string[] => {
      if (!parentId) return []
      return maps.childIdsMap.get(parentId) ?? []
    },
    [maps],
  )

  return (
    <CategoryNamesContext.Provider
      value={{ resolve, categoryType, parentName, childIdsOf, parentOrder: maps.parentOrder }}
    >
      {children}
    </CategoryNamesContext.Provider>
  )
}

function useCategoryNamesContext(): CategoryNamesContextValue {
  const ctx = useContext(CategoryNamesContext)
  if (!ctx) throw new Error("useCategoryNamesContext must be used inside CategoryNamesProvider")
  return ctx
}

export function useResolveCategoryName(): (id: string | null | undefined) => string {
  return useCategoryNamesContext().resolve
}

/** Full category metadata: name resolution, type lookup, parent name/order. */
export function useCategoryMeta(): CategoryNamesContextValue {
  return useCategoryNamesContext()
}
