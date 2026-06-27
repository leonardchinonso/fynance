import { cn } from "@/lib/utils"
import {
  Table, TableBody, TableCell, TableHead, TableHeader, TableRow,
} from "@/components/ui/table"

function Bone({ className, style }: { className?: string; style?: React.CSSProperties }) {
  return <div className={cn("animate-pulse rounded-md bg-muted", className)} style={style} />
}

// ─── Portfolio ────────────────────────────────────────────────────────────────

/** Portfolio Overview: net worth card, balance sheet, income card, pie, breakdowns */
export function PortfolioOverviewSkeleton() {
  return (
    <div className="space-y-4">
      <div className="grid gap-4 md:grid-cols-3">
        {/* Net worth card */}
        <div className="md:col-span-2 rounded-lg border p-6 space-y-4">
          <Bone className="h-3 w-20" />
          <Bone className="h-10 w-64" />
          <Bone className="h-3 w-48" />
          <div className="flex justify-between"><Bone className="h-4 w-32" /><Bone className="h-4 w-32" /></div>
          <Bone className="h-3 w-full rounded-full" />
        </div>
        {/* Balance sheet */}
        <div className="rounded-lg border p-6 space-y-4">
          <Bone className="h-3 w-24" />
          <div className="flex justify-between"><Bone className="h-5 w-16" /><Bone className="h-5 w-24" /></div>
          <div className="flex justify-between"><Bone className="h-5 w-16" /><Bone className="h-5 w-20" /></div>
          <Bone className="h-px w-full" />
          <div className="flex justify-between"><Bone className="h-5 w-12" /><Bone className="h-5 w-24" /></div>
        </div>
      </div>
      {/* Income + stocks */}
      <div className="grid gap-4 md:grid-cols-2">
        <div className="rounded-lg border p-6 space-y-4">
          <Bone className="h-3 w-40" />
          <div className="grid grid-cols-3 gap-4">
            {[1, 2, 3].map((i) => (<div key={i} className="space-y-2"><Bone className="h-3 w-16" /><Bone className="h-7 w-24" /><Bone className="h-3 w-20" /></div>))}
          </div>
          <Bone className="h-px w-full" />
          <div className="grid grid-cols-3 gap-4">
            {[1, 2, 3].map((i) => (<div key={i} className="space-y-2"><Bone className="h-3 w-20" /><Bone className="h-6 w-20" /></div>))}
          </div>
        </div>
        <div className="rounded-lg border p-6 space-y-4">
          <Bone className="h-3 w-28" />
          <div className="flex justify-center py-4"><Bone className="h-44 w-44 rounded-full" /></div>
          <div className="flex justify-center gap-3 flex-wrap">{[1, 2, 3, 4, 5].map((i) => <Bone key={i} className="h-3 w-16" />)}</div>
        </div>
      </div>
      {/* Breakdowns */}
      <div className="grid gap-4 md:grid-cols-3">
        {[1, 2, 3].map((i) => (
          <div key={i} className="rounded-lg border p-6 space-y-3">
            <Bone className="h-3 w-24" />
            {[1, 2, 3, 4].map((j) => (<div key={j} className="space-y-1.5"><div className="flex justify-between"><Bone className="h-3 w-20" /><Bone className="h-3 w-16" /></div><Bone className="h-1.5 w-full rounded-full" /></div>))}
          </div>
        ))}
      </div>
    </div>
  )
}

/** Portfolio Accounts: section headers + account cards */
export function AccountsGridSkeleton() {
  return (
    <div className="space-y-6">
      {["section-1", "section-2", "section-3"].map((section, si) => (
        <div key={section}>
          <Bone className="h-3 w-16 mb-3" />
          <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
            {Array.from({ length: si === 2 ? 2 : 3 }).map((_, i) => (
              <div key={i} className="rounded-lg border p-5 space-y-3">
                <div className="flex justify-between"><Bone className="h-4 w-28" /><Bone className="h-5 w-16 rounded-full" /></div>
                <Bone className="h-3 w-16" />
                <div className="flex items-baseline gap-2"><Bone className="h-7 w-32" /><Bone className="h-3 w-14" /></div>
                <Bone className="h-3 w-24" />
              </div>
            ))}
          </div>
        </div>
      ))}
    </div>
  )
}

/** Portfolio History: line chart with brush + table */
export function PortfolioHistorySkeleton() {
  return (
    <div className="space-y-6">
      {/* Chart card */}
      <ChartAreaSkeleton title={28} height={340} legendCount={3} />
      {/* Table */}
      <div className="rounded-lg border overflow-hidden">
        <table className="w-full">
          <thead>
            <tr className="border-b">
              {["w-16", "w-28", "w-32", "w-24", "w-16"].map((w, i) => (
                <th key={i} className="px-4 py-3 text-left"><Bone className={`h-3 ${w}`} /></th>
              ))}
            </tr>
          </thead>
          <tbody>
            {Array.from({ length: 8 }).map((_, i) => (
              <tr key={i} className="border-b">
                <td className="px-4 py-3"><Bone className="h-3 w-16" /></td>
                <td className="px-4 py-3"><Bone className="h-3 w-20" /></td>
                <td className="px-4 py-3"><Bone className="h-3 w-20" /></td>
                <td className="px-4 py-3"><Bone className="h-3 w-24" /></td>
                <td className="px-4 py-3"><Bone className="h-3 w-14" /></td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  )
}

// ─── Budget ──────────────────────────────────────────────────────────────────

/** Budget Spreadsheet: table with section headers, category rows, totals */
export function SpreadsheetSkeleton() {
  const cols = 7
  const sections = [{ rows: 1 }, { rows: 4 }, { rows: 6 }, { rows: 2 }]

  return (
    <div className="rounded-lg border overflow-hidden overflow-x-auto">
      <table className="w-full">
        <thead>
          <tr className="border-b">
            <th className="px-4 py-3 text-left w-[120px]"><Bone className="h-3 w-14" /></th>
            {Array.from({ length: cols }).map((_, i) => (
              <th key={i} className="px-3 py-3 text-right"><Bone className="h-3 w-8 ml-auto" /></th>
            ))}
            <th className="px-3 py-3 text-right"><Bone className="h-3 w-10 ml-auto" /></th>
            <th className="px-3 py-3 text-right"><Bone className="h-3 w-10 ml-auto" /></th>
          </tr>
        </thead>
        <tbody>
          {sections.map((section, si) => (
            <SpreadsheetSectionSkeleton key={si} rows={section.rows} cols={cols} />
          ))}
        </tbody>
      </table>
    </div>
  )
}

function SpreadsheetSectionSkeleton({ rows, cols }: { rows: number; cols: number }) {
  return (
    <>
      <tr className="bg-muted/50 border-b">
        <td colSpan={cols + 3} className="px-4 py-2"><Bone className="h-3 w-16" /></td>
      </tr>
      {Array.from({ length: rows }).map((_, ri) => (
        <tr key={ri} className="border-b">
          <td className="px-4 py-3"><Bone className={cn("h-3", ri % 2 === 0 ? "w-16" : "w-20")} /></td>
          {Array.from({ length: cols }).map((_, ci) => (
            <td key={ci} className="px-3 py-3 text-right"><Bone className={cn("h-3 ml-auto", ci % 3 === 0 ? "w-14" : "w-11")} /></td>
          ))}
          <td className="px-3 py-3 text-right"><Bone className="h-3 w-12 ml-auto" /></td>
          <td className="px-3 py-3 text-right"><Bone className="h-3 w-10 ml-auto" /></td>
        </tr>
      ))}
      <tr className="border-b-2">
        <td className="px-4 py-3"><Bone className="h-3 w-14" /></td>
        {Array.from({ length: cols }).map((_, ci) => (
          <td key={ci} className="px-3 py-3 text-right"><Bone className="h-3 w-14 ml-auto" /></td>
        ))}
        <td className="px-3 py-3 text-right"><Bone className="h-3 w-14 ml-auto" /></td>
        <td className="px-3 py-3" />
      </tr>
    </>
  )
}

/** Budget Charts: stacked bar + pie side by side, line below */
export function BudgetChartsSkeleton() {
  return (
    <div className="space-y-4">
      <div className="grid gap-4 lg:grid-cols-2">
        {/* Stacked bar */}
        <ChartAreaSkeleton title={44} height={320} legendCount={8} />
        {/* Pie */}
        <div className="rounded-lg border p-4 space-y-3">
          <Bone className="h-3 w-32" />
          <div className="flex justify-center py-6"><Bone className="h-56 w-56 rounded-full" /></div>
          <div className="flex flex-wrap justify-center gap-3">{Array.from({ length: 8 }).map((_, i) => <Bone key={i} className="h-3 w-20" />)}</div>
        </div>
      </div>
      {/* Line chart */}
      <ChartAreaSkeleton title={28} height={320} legendCount={8} />
    </div>
  )
}

// ─── Transactions ────────────────────────────────────────────────────────────

// Cell-content widths cycled through the skeleton so rows look varied, not striped.
const SKELETON_COL_WIDTHS = ["w-24", "w-16", "w-28", "w-20", "w-16", "w-24", "w-20", "w-16"]

/**
 * Generic loading state for a paginated table. Built from the real Table
 * primitives so padding, borders and row height line up with the loaded table,
 * with a matching pagination footer. `rows` should be the page size so the
 * placeholder fills the same space the data will. `actions` renders an
 * icon-button column (the dominant row-height driver on tables that have one);
 * `bordered` wraps it in a card border for tables rendered inside one.
 */
export function TableSkeleton({
  rows = 25,
  cols = 5,
  actions = true,
  bordered = false,
}: { rows?: number; cols?: number; actions?: boolean; bordered?: boolean }) {
  return (
    <div className={cn(bordered && "rounded-xl border overflow-hidden")}>
      <Table>
        <TableHeader>
          <TableRow>
            {Array.from({ length: cols }).map((_, i) => (
              <TableHead key={i}><Bone className="h-3 w-16" /></TableHead>
            ))}
            {actions && <TableHead className="text-right"><Bone className="h-3 w-12 ml-auto" /></TableHead>}
          </TableRow>
        </TableHeader>
        <TableBody>
          {Array.from({ length: rows }).map((_, r) => (
            <TableRow key={r}>
              {Array.from({ length: cols }).map((_, c) => (
                <TableCell key={c}>
                  <Bone className={cn("h-4", SKELETON_COL_WIDTHS[(r + c) % SKELETON_COL_WIDTHS.length])} />
                </TableCell>
              ))}
              {actions && (
                <TableCell>
                  <div className="flex items-center justify-end gap-1">
                    <Bone className="h-8 w-8 rounded-md" />
                    <Bone className="h-8 w-8 rounded-md" />
                  </div>
                </TableCell>
              )}
            </TableRow>
          ))}
        </TableBody>
      </Table>
      <div className="flex items-center justify-between border-t px-2 py-3">
        <Bone className="h-4 w-28" />
        <div className="flex items-center gap-2">
          <Bone className="h-4 w-24" />
          <Bone className="h-7 w-7 rounded-md" />
          <Bone className="h-7 w-7 rounded-md" />
        </div>
      </div>
    </div>
  )
}

/** Clean chart area skeleton: title, Y-axis, grid lines, X-axis, legend */
function ChartAreaSkeleton({ title, height = 320, legendCount = 4 }: { title: number; height?: number; legendCount?: number }) {
  return (
    <div className="rounded-lg border p-4 space-y-3">
      <Bone className="h-3" style={{ width: title * 4 }} />
      <div className="flex gap-2" style={{ height }}>
        <div className="flex flex-col justify-between py-2 w-16">
          {[1, 2, 3, 4, 5].map((i) => <Bone key={i} className="h-2.5 w-14 ml-auto" />)}
        </div>
        <div className="flex-1 relative">
          {[1, 2, 3, 4].map((i) => (<div key={i} className="absolute w-full border-t border-dashed border-muted" style={{ top: `${i * 20}%` }} />))}
        </div>
      </div>
      <div className="flex justify-between px-16">
        {[1, 2, 3, 4, 5, 6].map((i) => <Bone key={i} className="h-2.5 w-10" />)}
      </div>
      <div className="flex flex-wrap justify-center gap-3">
        {Array.from({ length: legendCount }).map((_, i) => <Bone key={i} className="h-3 w-16" />)}
      </div>
    </div>
  )
}

/** Generic chart skeleton for transaction bar/pie views */
export function ChartSkeleton({ height = 320 }: { height?: number }) {
  return <ChartAreaSkeleton title={32} height={height} legendCount={4} />
}

// ─── Settings ────────────────────────────────────────────────────────────────

/**
 * Generic settings list skeleton — shared by Profiles, Accounts, Categories,
 * and Ingestion sections. All four render as a vertical list of uniform bordered
 * rows with the same structure: icon + name + subtitle + badge + action buttons.
 */
export function SettingsListSkeleton({ rows = 4 }: { rows?: number }) {
  return (
    <div className="space-y-2">
      {Array.from({ length: rows }).map((_, i) => (
        <div key={i} className="flex items-center gap-3 rounded-lg border p-3">
          <Bone className="h-8 w-8 rounded-full shrink-0" />
          <div className="flex-1 space-y-1.5">
            <Bone className="h-3 w-32" />
            <Bone className="h-2.5 w-20" />
          </div>
          <Bone className="h-5 w-14 rounded-full" />
          <Bone className="h-7 w-7 rounded-md" />
          <Bone className="h-7 w-7 rounded-md" />
        </div>
      ))}
    </div>
  )
}
