import { api } from "@/api/client"
import type { Account } from "@/types"
import type { RemoteData } from "@/lib/remote_data"
import { useRemoteData } from "@/hooks/use_remote_data"

/**
 * Fetches accounts, optionally filtered by profile.
 *
 * - Hard dep: `profileId`
 * - No soft deps.
 *
 * Returns `[data, refresh]` — call `refresh()` after creating or modifying
 * an account to reload without changing any dep value.
 */
export function useAccounts(
  profileId?: string,
): [RemoteData<Account[]>, () => void] {
  return useRemoteData(
    () => api.getAccounts(profileId),
    { hard: [profileId], soft: [] },
  )
}
