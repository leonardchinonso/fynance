import { useState } from "react"
import type { PortfolioHistoryRow, Granularity } from "@/types"
import { visitRemoteData } from "@/lib/remote_data"
import { usePortfolioHistoryData } from "@/hooks/data"
import { PortfolioHistorySkeleton } from "@/components/skeletons"
import { AuthAwareError } from "@/components/auth_aware_error"
import { ReloadingOverlay } from "@/components/reloading_overlay"
import { StyledLineChart } from "@/components/charts"
import { EmptyState } from "@/components/empty_state"
import {
  Table, TableBody, TableCell, TableHead, TableHeader, TableRow,
} from "@/components/ui/table"
import { formatCurrency, formatMonth, getQuarter, getYear, periodKeyToRange } from "@/lib/utils"
import { DualAmount } from "@/components/currency"
import { cn } from "@/lib/utils"
import { usePreferredCurrency } from "@/context/preferred_currency_context"
import { useUrlFilters } from "@/hooks/use_url_filters"
import { useRedactedFlag } from "@/hooks/use_redacted_flag"
import { useChartContextMenu, ChartContextMenu } from "@/components/charts/chart_context_menu"

/**
 * History view. Loads its own data: portfolio history has exactly one consumer
 * (this view), so per the LCA rule the fetch lives here rather than at the page.
 * The page mounts this component only on the History tab, so no request is
 * issued until the tab is shown.
 */
export function PortfolioHistory({
  start,
  end,
  granularity,
  profileId,
}: {
  start: string
  end: string
  granularity: Granularity
  profileId: string | undefined
}) {
  const data = usePortfolioHistoryData(start, end, granularity, profileId)
  return visitRemoteData(data, {
    notLoaded: () => <PortfolioHistorySkeleton />,
    failed: (error) => <AuthAwareError error={error} />,
    hasValue: (history) => (
      <div className="relative">
        <PortfolioHistoryInternal history={history} granularity={granularity} />
        <ReloadingOverlay active={data.status === "reloading"} />
      </div>
    ),
  })
}

interface PortfolioHistoryProps {
  history: PortfolioHistoryRow[]
  granularity: Granularity
}

function formatPeriodLabel(key: string, granularity: Granularity): string {
  if (granularity === "monthly") return formatMonth(key)
  // Backend quarterly labels are "YYYY-Qn"; render as "Qn YYYY". Yearly ("YYYY")
  // and already-formatted "Qn YYYY" keys pass through unchanged.
  const q = key.match(/^(\d{4})-Q(\d)$/)
  if (q) return `Q${q[2]} ${q[1]}`
  return key
}

function aggregateHistory(
  history: PortfolioHistoryRow[],
  granularity: Granularity
): PortfolioHistoryRow[] {
  if (granularity === "monthly") return history

  // The backend already returns rows bucketed by granularity ("YYYY-Qn" for
  // quarterly, "YYYY" for yearly), so there is nothing to re-aggregate. Only the
  // mock returns raw monthly ("YYYY-MM") rows that still need grouping here;
  // re-bucketing the backend's labels with getQuarter would produce "QNaN".
  const isRawMonthly = history.length === 0 || /^\d{4}-\d{2}$/.test(history[0].month)
  if (!isRawMonthly) return history

  const keyFn = granularity === "quarterly" ? getQuarter : getYear
  const groups = new Map<
    string,
    { available: number; unavailable: number; count: number }
  >()
  const orderedKeys: string[] = []

  for (const row of history) {
    const key = keyFn(row.month)
    if (!groups.has(key)) {
      groups.set(key, { available: 0, unavailable: 0, count: 0 })
      orderedKeys.push(key)
    }
    const g = groups.get(key)!
    g.available = parseFloat(row.available_wealth)
    g.unavailable = parseFloat(row.unavailable_wealth)
    g.count++
  }

  return orderedKeys.map((key) => {
    const g = groups.get(key)!
    return {
      month: key,
      available_wealth: g.available.toFixed(2),
      available_wealth_display: null,
      unavailable_wealth: g.unavailable.toFixed(2),
      unavailable_wealth_display: null,
      total_wealth: (g.available + g.unavailable).toFixed(2),
      total_wealth_display: null,
    }
  })
}

function PortfolioHistoryInternal({ history, granularity }: PortfolioHistoryProps) {
  useRedactedFlag()
  const preferredCurrency = usePreferredCurrency()
  const [hoveredIndex, setHoveredIndex] = useState<number | null>(null)
  const { setFilter } = useUrlFilters()
  const { menu, open, close } = useChartContextMenu()

  const aggregated = aggregateHistory(history, granularity)

  // Respect the selected date range: keep every period the backend returned and
  // render points before the first non-zero value as gaps (null) rather than
  // dropping them, so the axis spans the chosen range and the line simply starts
  // where tracking begins.
  const firstIdx = aggregated.findIndex((row) => parseFloat(row.total_wealth) > 0)

  if (firstIdx === -1) {
    return <EmptyState />
  }

  const chartData = aggregated.map((row, i) => {
    const tracked = i >= firstIdx
    return {
      period: formatPeriodLabel(row.month, granularity),
      Available: tracked ? parseFloat(row.available_wealth) : null,
      Unavailable: tracked ? parseFloat(row.unavailable_wealth) : null,
      Total: tracked ? parseFloat(row.total_wealth) : null,
    }
  })

  // The table lists only periods from the first tracked value onward; leading
  // empties would just be rows of £0.00.
  const tableData = aggregated.slice(firstIdx)

  const periodLabel =
    granularity === "monthly"
      ? "Month"
      : granularity === "quarterly"
        ? "Quarter"
        : "Year"

  const handleContextMenu = (
    e: { clientX: number; clientY: number; preventDefault: () => void },
    ctx: { index: number | null },
  ) => {
    if (ctx.index == null) return
    const row = aggregated[ctx.index]
    if (!row) return
    const label = formatPeriodLabel(row.month, granularity)
    const range = periodKeyToRange(row.month, granularity)
    open(e, [{
      label: `Open ${label} in Accounts`,
      onSelect: () => setFilter({ view: "accounts", preset: "custom", start: range.start, end: range.end }),
    }])
  }

  return (
    <div className="space-y-6">
      <div className="rounded-lg border p-4">
        <div className="mb-2 flex items-center justify-between">
          <h3 className="text-sm font-medium text-muted-foreground">
            Portfolio History
          </h3>
        </div>
        <StyledLineChart
          data={chartData}
          index="period"
          categories={["Total", "Available", "Unavailable"]}
          colors={["#22c55e", "#3b82f6", "#f97316"]}
          height={340}
          curved
          highlightIndex={hoveredIndex}
          onActiveIndexChange={setHoveredIndex}
          onContextMenu={handleContextMenu}
        />
        <ChartContextMenu menu={menu} onClose={close} />
      </div>

      <div className="overflow-x-auto rounded-lg border">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>{periodLabel}</TableHead>
              <TableHead className="text-right">Available Wealth</TableHead>
              <TableHead className="text-right">Unavailable Wealth</TableHead>
              <TableHead className="text-right">Total Wealth</TableHead>
              <TableHead className="text-right">Change</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {[...tableData].reverse().map((row, i, reversed) => {
              const prevRow = i < reversed.length - 1 ? reversed[i + 1] : null
              const change = prevRow
                ? parseFloat(row.total_wealth) - parseFloat(prevRow.total_wealth)
                : null
              const chartIndex = aggregated.length - 1 - i

              return (
                <TableRow
                  key={row.month}
                  className={cn(
                    "cursor-pointer transition-colors",
                    hoveredIndex === chartIndex && "bg-muted/50"
                  )}
                  onMouseEnter={() => setHoveredIndex(chartIndex)}
                  onMouseLeave={() => setHoveredIndex(null)}
                >
                  <TableCell className="font-medium">
                    {formatPeriodLabel(row.month, granularity)}
                  </TableCell>
                  <TableCell className="text-right">
                    <DualAmount value={row.available_wealth} preferredCurrency={preferredCurrency} display={row.available_wealth_display} tooltip />
                  </TableCell>
                  <TableCell className="text-right">
                    <DualAmount value={row.unavailable_wealth} preferredCurrency={preferredCurrency} display={row.unavailable_wealth_display} tooltip />
                  </TableCell>
                  <TableCell className="text-right font-medium">
                    <DualAmount value={row.total_wealth} preferredCurrency={preferredCurrency} display={row.total_wealth_display} tooltip />
                  </TableCell>
                  <TableCell className="text-right tabular-nums">
                    {change !== null ? (
                      <span
                        className={cn(
                          "text-sm",
                          change >= 0 ? "text-green-500" : "text-red-500"
                        )}
                      >
                        {change >= 0 ? "+" : ""}
                        {formatCurrency(change.toFixed(2), preferredCurrency)}
                      </span>
                    ) : (
                      <span className="text-muted-foreground">-</span>
                    )}
                  </TableCell>
                </TableRow>
              )
            })}
          </TableBody>
        </Table>
      </div>
    </div>
  )
}
