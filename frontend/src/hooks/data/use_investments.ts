import { api } from "@/api/client"
import type { InvestmentEvent } from "@/bindings/InvestmentEvent"
import type { S104PoolState } from "@/bindings/S104PoolState"
import type { RemoteData } from "@/lib/remote_data"
import { useRemoteData } from "@/hooks/use_remote_data"

/**
 * Fetches the full investment-events ledger for the profile, unfiltered.
 *
 * All filtering (account, type, symbol, search, date) is done client-side by
 * the History table, so the ledger is fetched once and reused. This keeps the
 * History view from re-fetching on every filter change and lets it paginate
 * the full result set locally.
 *
 * Returns `[data, reload]` — call `reload()` after creating, editing, or
 * deleting an event to refresh.
 */
export function useInvestments(): [RemoteData<InvestmentEvent[]>, () => void] {
  return useRemoteData(
    () => api.listInvestments(),
    { hard: [], soft: [] },
  )
}

/**
 * Fetches the S104 average-cost pool snapshot per symbol.
 *
 * Returns `[data, reload]` — call `reload()` after mutating events so the
 * cost-basis summary reflects the change.
 */
export function useInvestmentPools(): [RemoteData<S104PoolState[]>, () => void] {
  return useRemoteData(() => api.getInvestmentPools(), { hard: [], soft: [] })
}
