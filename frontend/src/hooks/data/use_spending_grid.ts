import { api } from "@/api/client"
import type { Granularity, SpendingGridRow } from "@/types"
import type { RemoteData } from "@/lib/remote_data"
import { useRemoteData } from "@/hooks/use_remote_data"

/**
 * Fetches the spending grid (budget spreadsheet rows).
 *
 * - Hard dep: `profileId` — switching profile wipes the grid and shows a skeleton.
 * - Soft deps: `start`, `end`, `granularity` — date/view changes keep the old grid
 *   visible while new data loads.
 *
 * Returns `[data, refresh]` — call `refresh()` after a budget mutation to reload
 * the grid without changing any filter value.
 */
export function useSpendingGrid(
  start: string,
  end: string,
  granularity: Granularity,
  profileId: string | undefined,
): [RemoteData<SpendingGridRow[]>, () => void] {
  return useRemoteData(
    () => api.getSpendingGrid(start, end, granularity, profileId),
    { hard: [profileId], soft: [start, end, granularity] },
  )
}
