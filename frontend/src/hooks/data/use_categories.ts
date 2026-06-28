import { api } from "@/api/client"
import type { CategoryNode } from "@/bindings/CategoryNode"
import type { RemoteData } from "@/lib/remote_data"
import { useQuery } from "@/hooks/use_query"

/**
 * Fetches the full category tree for the Settings > Categories section.
 *
 * Reference data, so it is cached session-stable; mutations to categories
 * invalidate it explicitly via the api client. Returns `[data, refresh]` — call
 * `refresh()` to force a reload without changing any dep value.
 */
export function useCategories(): [RemoteData<CategoryNode[]>, () => void] {
  return useQuery(
    () => api.getCategoryDetails(),
    { tag: "categories", hard: [], soft: [], static: true },
  )
}
