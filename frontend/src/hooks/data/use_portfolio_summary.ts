import { api } from "@/api/client"
import type { Currency, Granularity, Holding, PortfolioHistoryRow, PortfolioResponse } from "@/types"
import type { RemoteData } from "@/lib/remote_data"
import { RemoteData as RD, combineRemoteData } from "@/lib/remote_data"
import { useQuery } from "@/hooks/use_query"

/** Data needed by the Overview and Charts views. */
export interface PortfolioSummaryData {
  portfolio: PortfolioResponse
  /** Used in Overview for start/end net worth delta over the selected period. */
  history: PortfolioHistoryRow[]
  /** Holdings for all accounts (empty array for accounts with no positions). */
  allHoldings: Holding[]
  /** FX rates keyed by currency code, for converting holding values. */
  currencies: Currency[]
}

/**
 * Composes the Overview data set from per-endpoint cache entries, so each
 * piece is cached and deduped independently: a date-range change refetches
 * only the history, and `currencies`/`holdings-history` entries are shared
 * with every other consumer of the same request shape.
 *
 * The holdings batch depends on the summary's account list, so it starts once
 * the summary lands (and is skipped entirely for zero accounts).
 */
export function usePortfolioSummary(
  start: string,
  end: string,
  granularity: Granularity,
  profileId: string | undefined,
  enabled = true,
): RemoteData<PortfolioSummaryData> {
  const [portfolio] = useQuery(
    () => api.getPortfolio(profileId),
    { tag: "holdings-summary", hard: [profileId], soft: [], enabled },
  )
  const [history] = useQuery(
    () => api.getPortfolioHistory(start, end, granularity, profileId),
    { tag: "holdings-history", hard: [profileId], soft: [start, end, granularity], enabled },
  )
  const [currencies] = useQuery(
    () => api.getCurrencies(),
    { tag: "currencies", hard: [], soft: [], static: true, enabled },
  )

  const accountIds =
    portfolio.status === "succeeded" || portfolio.status === "reloading"
      ? portfolio.value.accounts.map((a) => a.id)
      : null
  const [holdingsBatch] = useQuery(
    () => api.getHoldingsBatch(accountIds ?? []),
    {
      tag: "holdings-batch",
      hard: [],
      soft: [accountIds],
      enabled: enabled && accountIds !== null && accountIds.length > 0,
    },
  )
  const holdings: RemoteData<Holding[]> =
    accountIds !== null && accountIds.length === 0 ? RD.succeeded([]) : holdingsBatch

  return combineRemoteData(
    [portfolio, history, holdings, currencies] as const,
    ([p, h, hold, c]) => ({ portfolio: p, history: h, allHoldings: hold, currencies: c }),
  )
}
