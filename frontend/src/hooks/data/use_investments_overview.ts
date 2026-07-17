import { api } from "@/api/client"
import type { Account, Currency, Holding } from "@/types"
import type { InvestmentEvent } from "@/bindings/InvestmentEvent"
import type { InvestmentHistoryRow } from "@/bindings/InvestmentHistoryRow"
import type { S104PoolState } from "@/bindings/S104PoolState"
import type { CgtSummary } from "@/bindings/CgtSummary"
import type { RemoteData } from "@/lib/remote_data"
import { sumMoney } from "@/lib/money"
import { useQuery } from "@/hooks/use_query"
import { accountTypeToAssetClass } from "@/lib/account_type_utils"

/** Everything the Investments Overview dashboard needs, fetched in parallel. */
export interface InvestmentsOverviewData {
  /** Holdings across investment-class accounts only (market value). */
  holdings: Holding[]
  /** S104 cost-basis pools per symbol (already in base currency). */
  pools: S104PoolState[]
  /** Full events ledger, for the cumulative-invested time series. */
  events: InvestmentEvent[]
  /** Per-period net invested vs market value (investment + ISA), for the chart. */
  investmentHistory: InvestmentHistoryRow[]
  /** FX rates keyed by currency code, for converting holding values. */
  currencies: Currency[]
  /**
   * Realised gains for the selected period. For a single profile this is its CGT
   * summary; for "all profiles" it is every profile's summary summed. `null` only
   * when there are no profiles or no disposals in range.
   */
  realisedGains: CgtSummary | null
  preferredCurrency: string
}

function preferredCode(currencies: Currency[]): string {
  return currencies.find((c) => c.is_preferred)?.code ?? "GBP"
}

/** Sum CGT summaries field-by-field (realised gains are additive across profiles). */
function sumCgtSummaries(items: CgtSummary[]): CgtSummary | null {
  if (items.length === 0) return null
  const sum = (pick: (s: CgtSummary) => string) => sumMoney(items.map(pick))
  return {
    total_proceeds: sum((s) => s.total_proceeds),
    total_allowable_costs: sum((s) => s.total_allowable_costs),
    total_gains: sum((s) => s.total_gains),
    total_losses: sum((s) => s.total_losses),
    net_gain_loss: sum((s) => s.net_gain_loss),
    base_currency: items[0].base_currency,
  }
}

/**
 * Composes the Investments Overview data set.
 *
 * - Hard dep: `profileId` (identity change — clears stale data on switch)
 * - Soft deps: `start`, `end` (scope the realised-gains period), the selected
 *   account ids (scope holdings + events).
 *
 * When `selectedAccountIds` is non-empty, holdings (current value + pie) and
 * events (cost basis + invested line) are scoped to those accounts. Pools and
 * realised gains stay profile-scoped: pools have no account_id, and the CGT
 * report has no per-account filter.
 *
 * Realised gains come from the CGT report, which is per-profile: for a single
 * profile we fetch its summary; for "all profiles" we fetch each profile's
 * summary and sum them (realised gains are additive across profiles).
 */
export function useInvestmentsOverview(
  start: string,
  end: string,
  profileId: string | undefined,
  selectedAccountIds: string[] = [],
  enabled = true,
): RemoteData<InvestmentsOverviewData> {
  const selectedKey = [...selectedAccountIds].sort().join(",")
  const [data] = useQuery(
    async (): Promise<InvestmentsOverviewData> => {
      const [accounts, pools, allEvents, currencies] = await Promise.all([
        api.getAccounts(profileId),
        api.getInvestmentPools(profileId),
        api.listInvestments(),
        api.getCurrencies(),
      ])

      const selected = new Set(selectedAccountIds)
      const investmentAccountIds = accounts
        .filter((a: Account) => accountTypeToAssetClass(a.type) === "Investments")
        .filter((a) => selected.size === 0 || selected.has(a.id))
        .map((a) => a.id)
      const accountIdSet = new Set(investmentAccountIds)

      // Scope the chart to the same accounts as the holdings and events. Passing
      // nothing here would plot the whole profile against a filtered pie.
      const investmentHistory = await api.getInvestmentHistory(
        start,
        end,
        "monthly",
        profileId,
        selected.size === 0 ? [] : investmentAccountIds,
      )

      // Scope events to the same investment accounts as the holdings, so cost
      // basis (derived from events) and current value (from holdings) cover the
      // same set. Without this, selecting a profile compares that profile's
      // holdings against every profile's events.
      const events = allEvents.filter((e) => accountIdSet.has(e.account_id))

      const holdings = investmentAccountIds.length > 0
        ? await api.getHoldingsBatch(investmentAccountIds)
        : []

      const cgtPeriod = { kind: "range" as const, startDate: start, endDate: end }
      let realisedGains: CgtSummary | null = null
      if (profileId) {
        const cgt = await api.getCapitalGains({ period: cgtPeriod, profileId })
        realisedGains = cgt.summary
      } else {
        // "All profiles": the CGT report is per-profile, so fetch each profile's
        // summary and sum them.
        const profiles = await api.getProfiles()
        const summaries = await Promise.all(
          profiles.map((p) =>
            api.getCapitalGains({ period: cgtPeriod, profileId: p.id })
              .then((c) => c.summary)
              .catch(() => null),
          ),
        )
        realisedGains = sumCgtSummaries(summaries.filter((s): s is CgtSummary => s !== null))
      }

      return {
        holdings,
        pools,
        events,
        investmentHistory,
        currencies,
        realisedGains,
        preferredCurrency: preferredCode(currencies),
      }
    },
    { tag: "investments-overview", hard: [profileId], soft: [start, end, selectedKey], enabled },
  )
  return data
}
