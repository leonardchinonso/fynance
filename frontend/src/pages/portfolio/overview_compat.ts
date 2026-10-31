import type { CashSummaryResponse, HoldingsHistoryRow } from "@/types"

// The frontend auto-deploys to Vercel on merge to master, but the backend
// only updates when a release is cut and the server is updated. In the
// window between the two, live serves this frontend against the previous
// backend, which omits fields added by that release. These readers fall
// back to the pre-release behaviour instead of rendering NaN or blank.

/**
 * Opening net worth for the period delta. New backends send
 * `period_start_wealth` (the value entering day 1); older ones do not, and
 * the opening used to be the first bucket's total.
 */
export function openingNetWorth(
  periodStartWealth: string | undefined,
  history: HoldingsHistoryRow[],
): string | undefined {
  return periodStartWealth ?? history[0]?.total_wealth
}

/**
 * The Net Savings figure. New backends return `net_savings` (income minus
 * spending); older ones return `savings_growth` (the change in savings and
 * emergency-fund balances), which is what the card showed before.
 */
export function cashSummaryNetSavings(cashSummary: CashSummaryResponse): string {
  const wire = cashSummary as Partial<CashSummaryResponse> & {
    net_savings?: string
    savings_growth?: string
  }
  return wire.net_savings ?? wire.savings_growth ?? "0"
}
