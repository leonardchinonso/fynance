import { useMemo } from "react"
import { useCategories } from "./use_categories"

export interface CategoryOption {
  id: string
  name: string
}

/**
 * Leaf categories as `{id, name}` with `"Parent: Child"` names, derived from
 * the shared cached category tree (same cache entry as {@link useCategories}),
 * so category mutations invalidate it automatically. Empty until loaded.
 */
export function useCategoryOptions(): CategoryOption[] {
  const [data] = useCategories()
  const nodes =
    data.status === "succeeded" || data.status === "reloading" ? data.value : null
  return useMemo(() => {
    if (!nodes) return []
    return nodes.flatMap((node) => {
      const children = node.children ?? []
      if (children.length === 0) return [{ id: node.id, name: node.name }]
      return children.map((c) => ({ id: c.id, name: `${node.name}: ${c.name}` }))
    })
  }, [nodes])
}
