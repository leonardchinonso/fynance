import { api } from "@/api/client"
import type { Profile } from "@/types"
import type { RemoteData } from "@/lib/remote_data"
import { useRemoteData } from "@/hooks/use_remote_data"

/**
 * Fetches all profiles. No deps — profiles are global and fetched once on mount.
 *
 * Returns `[data, refresh]` — call `refresh()` after creating a profile.
 */
export function useProfilesData(): [RemoteData<Profile[]>, () => void] {
  return useRemoteData(
    () => api.getProfiles(),
    { hard: [], soft: [] },
  )
}
