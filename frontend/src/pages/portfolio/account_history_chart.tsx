import type { Granularity } from "@/types"
import type { AccountHoldingsHistory } from "@/api/service"
import { visitRemoteData } from "@/lib/remote_data"
import { useAccountHoldingsHistory } from "@/hooks/data"
import { PortfolioHistorySkeleton } from "@/components/skeletons"
import { AuthAwareError } from "@/components/auth_aware_error"
import { ReloadingOverlay } from "@/components/reloading_overlay"
import { StyledLineChart } from "@/components/charts"
import { EmptyState } from "@/components/empty_state"
import { formatMonth } from "@/lib/utils"

// Total line is green (matches the Portfolio History page); individual holdings
// cycle through this palette; the collapsed "Other" line is muted.
const TOTAL_COLOR = "#22c55e"
const OTHER_COLOR = "#78716c"
const HOLDING_COLORS = [
  "#3b82f6", "#f97316", "#a855f7", "#ec4899", "#06b6d4",
  "#eab308", "#6366f1", "#14b8a6", "#ef4444", "#f59e0b",
]

// Cap the number of individual holding lines; the rest collapse into "Other".
const TOP_N = 8

/** One holding present at the hovered period, with its as-of value (preferred
 * currency) and the display metadata needed to render a table row. */
export interface HoverHolding {
  symbol: string
  name: string
  shortName: string | null
  holdingType: string
  value: number
}

/** The period the cursor is over. `holdings` is exactly the set of positions
 * open at that period (sorted by value, descending), so the holdings table can
 * add/remove rows as the cursor moves, not just restate the current set. */
export interface HoverPeriod {
  label: string
  total: number
  holdings: HoverHolding[]
}

interface AccountHistoryChartProps {
  accountId: string
  start: string
  end: string
  granularity: Granularity
  onHoverPeriod?: (period: HoverPeriod | null) => void
}

function formatPeriodLabel(period: string, granularity: Granularity): string {
  // Backend labels: "YYYY-MM" | "YYYY-Qn" | "YYYY". Quarter/year are already readable.
  return granularity === "monthly" ? formatMonth(period) : period
}

export function AccountHistoryChart({ accountId, start, end, granularity, onHoverPeriod }: AccountHistoryChartProps) {
  const data = useAccountHoldingsHistory(accountId, start, end, granularity)

  return visitRemoteData(data, {
    notLoaded: () => <PortfolioHistorySkeleton />,
    failed: (error) => <AuthAwareError error={error} />,
    hasValue: (history) => (
      <div className="relative">
        <AccountHistoryChartInternal history={history} granularity={granularity} onHoverPeriod={onHoverPeriod} />
        <ReloadingOverlay active={data.status === "reloading"} />
      </div>
    ),
  })
}

function AccountHistoryChartInternal({
  history,
  granularity,
  onHoverPeriod,
}: {
  history: AccountHoldingsHistory
  granularity: Granularity
  onHoverPeriod?: (period: HoverPeriod | null) => void
}) {
  const { symbols, rows } = history

  const hasData = symbols.length > 0 && rows.some((r) => parseFloat(r.total) !== 0)
  if (!hasData) return <EmptyState />

  const seriesBySymbol = new Map(symbols.map((s) => [s.symbol, s]))

  // Rank holdings by their value in the latest period; keep the top N as their
  // own lines, collapse the rest into a single "Other" line.
  const latestRow = rows[rows.length - 1]
  const latestValue = new Map(latestRow.values.map((v) => [v.symbol, parseFloat(v.value)]))
  const ranked = [...symbols].sort(
    (a, b) => (latestValue.get(b.symbol) ?? 0) - (latestValue.get(a.symbol) ?? 0),
  )
  const top = ranked.slice(0, TOP_N)
  const rest = ranked.slice(TOP_N)

  // Stable, unique display names for the top holdings.
  const usedNames = new Set<string>()
  const nameBySymbol = new Map<string, string>()
  for (const s of top) {
    let name = s.short_name ?? s.symbol
    if (usedNames.has(name)) name = `${name} (${s.symbol})`
    usedNames.add(name)
    nameBySymbol.set(s.symbol, name)
  }

  const chartData = rows.map((row) => {
    const valueBySymbol = new Map(row.values.map((v) => [v.symbol, parseFloat(v.value)]))
    // A symbol absent from `values` has no open (non-closed) snapshot at/before this
    // period: render a gap (null), not 0, so each line starts where tracking begins
    // and breaks again once the position is closed.
    const tracked = row.values.length > 0
    const point: Record<string, string | number | null> = {
      period: formatPeriodLabel(row.period, granularity),
      Total: tracked ? parseFloat(row.total) : null,
    }
    for (const s of top) {
      point[nameBySymbol.get(s.symbol)!] = valueBySymbol.has(s.symbol) ? valueBySymbol.get(s.symbol)! : null
    }
    if (rest.length > 0) {
      const present = rest.filter((s) => valueBySymbol.has(s.symbol))
      point.Other = present.length > 0
        ? present.reduce((sum, s) => sum + valueBySymbol.get(s.symbol)!, 0)
        : null
    }
    return point
  })

  const categories = [
    "Total",
    ...top.map((s) => nameBySymbol.get(s.symbol)!),
    ...(rest.length > 0 ? ["Other"] : []),
  ]
  const colors = [
    TOTAL_COLOR,
    ...top.map((_, i) => HOLDING_COLORS[i % HOLDING_COLORS.length]),
    ...(rest.length > 0 ? [OTHER_COLOR] : []),
  ]

  return (
    <div className="rounded-lg border p-4">
      <h3 className="mb-2 text-sm font-medium text-muted-foreground">Value history</h3>
      <StyledLineChart
        data={chartData}
        index="period"
        categories={categories}
        colors={colors}
        height={320}
        curved
        onActiveIndexChange={(i) => {
          if (!onHoverPeriod) return
          if (i === null || !rows[i]) {
            onHoverPeriod(null)
            return
          }
          const row = rows[i]
          const holdings = row.values
            .map((v) => {
              const meta = seriesBySymbol.get(v.symbol)
              return {
                symbol: v.symbol,
                name: meta?.name ?? v.symbol,
                shortName: meta?.short_name ?? null,
                holdingType: meta?.holding_type ?? "stock",
                value: parseFloat(v.value),
              }
            })
            .sort((a, b) => b.value - a.value)
          onHoverPeriod({
            label: formatPeriodLabel(row.period, granularity),
            total: parseFloat(row.total),
            holdings,
          })
        }}
      />
    </div>
  )
}
