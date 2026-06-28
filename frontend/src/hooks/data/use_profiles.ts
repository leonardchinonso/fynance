import { api } from "@/api/client"
import type { Profile } from "@/types"
import type { RemoteData } from "@/lib/remote_data"
import { useQuery } from "@/hooks/use_query"

/**
 * Fetches all profiles. Global reference data, cached session-stable; profile
 * mutations invalidate it via the api client.
 *
 * Returns `[data, refresh]` — call `refresh()` after creating a profile.
 */
export function useProfilesData(): [RemoteData<Profile[]>, () => void] {
  return useQuery(
    () => api.getProfiles(),
    { tag: "profiles", hard: [], soft: [], static: true },
  )
}
