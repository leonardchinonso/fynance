import { api } from "@/api/client"
import type { InvestmentEvent } from "@/bindings/InvestmentEvent"
import type { S104PoolState } from "@/bindings/S104PoolState"
import type { RemoteData } from "@/lib/remote_data"
import { useQuery } from "@/hooks/use_query"

/**
 * Fetches the full investment-events ledger for the profile, unfiltered.
 *
 * All filtering, sorting, and pagination happen client-side over the full
 * ledger, deliberately diverging from transactions (which paginate
 * server-side): event counts are small, and several views reuse this one
 * cached list.
 *
 * `enabled` gates the fetch so the History tab issues no request until shown.
 * Returns `[data, reload]` — call `reload()` after creating, editing, or
 * deleting an event to refresh.
 */
export function useInvestments(enabled = true): [RemoteData<InvestmentEvent[]>, () => void] {
  return useQuery(
    () => api.listInvestments(),
    { tag: "investments", hard: [], soft: [], enabled },
  )
}

/**
 * Fetches the S104 average-cost pool snapshot per symbol.
 *
 * Returns `[data, reload]` — call `reload()` after mutating events so the
 * cost-basis summary reflects the change.
 */
export function useInvestmentPools(): [RemoteData<S104PoolState[]>, () => void] {
  return useQuery(() => api.getInvestmentPools(), { tag: "investment-pools", hard: [], soft: [] })
}
