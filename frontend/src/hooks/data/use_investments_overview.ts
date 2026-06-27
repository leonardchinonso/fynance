import { api } from "@/api/client"
import type { Account, Currency, Holding } from "@/types"
import type { InvestmentEvent } from "@/bindings/InvestmentEvent"
import type { S104PoolState } from "@/bindings/S104PoolState"
import type { CgtSummary } from "@/bindings/CgtSummary"
import type { RemoteData } from "@/lib/remote_data"
import { useRemoteData } from "@/hooks/use_remote_data"
import { accountTypeToAssetClass } from "@/lib/account_type_utils"

/** Everything the Investments Overview dashboard needs, fetched in parallel. */
export interface InvestmentsOverviewData {
  /** Holdings across investment-class accounts only (market value). */
  holdings: Holding[]
  /** S104 cost-basis pools per symbol (already in base currency). */
  pools: S104PoolState[]
  /** Full events ledger, for the cumulative-invested time series. */
  events: InvestmentEvent[]
  /** FX rates keyed by currency code, for converting holding values. */
  currencies: Currency[]
  /**
   * Realised gains for the selected period. `null` when no single profile is
   * selected (the CGT report has no "all profiles" mode).
   */
  realisedGains: CgtSummary | null
  preferredCurrency: string
}

function preferredCode(currencies: Currency[]): string {
  return currencies.find((c) => c.is_preferred)?.code ?? "GBP"
}

/**
 * Composes the Investments Overview data set.
 *
 * - Hard dep: `profileId` (identity change — clears stale data on switch)
 * - Soft deps: `start`, `end` (scope the realised-gains period)
 *
 * Realised gains are only fetched when a single `profileId` is selected; the
 * CGT report has no all-profiles mode, so it is left `null` otherwise and the
 * caller renders a "select a profile" hint.
 */
export function useInvestmentsOverview(
  start: string,
  end: string,
  profileId: string | undefined,
): RemoteData<InvestmentsOverviewData> {
  const [data] = useRemoteData(
    async (): Promise<InvestmentsOverviewData> => {
      const [accounts, pools, allEvents, currencies] = await Promise.all([
        api.getAccounts(profileId),
        api.getInvestmentPools(profileId),
        api.listInvestments(),
        api.getCurrencies(),
      ])

      const investmentAccountIds = accounts
        .filter((a: Account) => accountTypeToAssetClass(a.type) === "Investments")
        .map((a) => a.id)
      const accountIdSet = new Set(investmentAccountIds)

      // Scope events to the same investment accounts as the holdings, so cost
      // basis (derived from events) and current value (from holdings) cover the
      // same set. Without this, selecting a profile compares that profile's
      // holdings against every profile's events.
      const events = allEvents.filter((e) => accountIdSet.has(e.account_id))

      const holdings = investmentAccountIds.length > 0
        ? await api.getHoldingsBatch(investmentAccountIds)
        : []

      let realisedGains: CgtSummary | null = null
      if (profileId) {
        const cgt = await api.getCapitalGains({
          period: { kind: "range", startDate: start, endDate: end },
          profileId,
        })
        realisedGains = cgt.summary
      }

      return {
        holdings,
        pools,
        events,
        currencies,
        realisedGains,
        preferredCurrency: preferredCode(currencies),
      }
    },
    { hard: [profileId], soft: [start, end] },
  )
  return data
}
