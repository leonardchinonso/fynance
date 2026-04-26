import { api } from "@/api/client"
import type { CategoryDetail } from "@/types"
import type { RemoteData } from "@/lib/remote_data"
import { useRemoteData } from "@/hooks/use_remote_data"

/**
 * Fetches the full category list for the Settings > Categories section.
 *
 * Returns `[data, refresh]` — call `refresh()` after a create/update/delete
 * mutation to reload without changing any dep value.
 */
export function useCategories(): [RemoteData<CategoryDetail[]>, () => void] {
  return useRemoteData(
    () => api.getCategoryDetails(),
    { hard: [], soft: [] },
  )
}
