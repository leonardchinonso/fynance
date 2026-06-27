import { api } from "@/api/client"
import type { InvestmentEvent } from "@/bindings/InvestmentEvent"
import type { S104PoolState } from "@/bindings/S104PoolState"
import type { RemoteData } from "@/lib/remote_data"
import { useRemoteData } from "@/hooks/use_remote_data"

export interface InvestmentFilters {
  accountId?: string
  symbol?: string
  eventType?: string
}

/**
 * Fetches the investment-events ledger, optionally filtered.
 *
 * - Soft deps: `accountId`, `symbol`, `eventType` (filter changes keep the
 *   previous rows visible while reloading).
 *
 * Returns `[data, reload]` — call `reload()` after creating, editing, or
 * deleting an event to refresh without changing any dep value.
 */
export function useInvestments(
  filters: InvestmentFilters = {},
): [RemoteData<InvestmentEvent[]>, () => void] {
  const { accountId, symbol, eventType } = filters
  return useRemoteData(
    () => api.listInvestments(accountId, symbol, eventType),
    { hard: [], soft: [accountId, symbol, eventType] },
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
