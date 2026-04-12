import { cn } from "@/lib/utils"

function Bone({ className }: { className?: string }) {
  return (
    <div
      className={cn(
        "animate-pulse rounded-md bg-muted",
        className
      )}
    />
  )
}

/** Skeleton for the portfolio overview page */
export function PortfolioOverviewSkeleton() {
  return (
    <div className="space-y-4">
      {/* Net worth + balance sheet */}
      <div className="grid gap-4 md:grid-cols-3">
        <div className="md:col-span-2 rounded-lg border p-6 space-y-4">
          <Bone className="h-3 w-20" />
          <Bone className="h-10 w-64" />
          <Bone className="h-3 w-48" />
          <div className="flex justify-between">
            <Bone className="h-4 w-32" />
            <Bone className="h-4 w-32" />
          </div>
          <Bone className="h-3 w-full" />
        </div>
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
            {[1, 2, 3].map((i) => (
              <div key={i} className="space-y-2">
                <Bone className="h-3 w-16" />
                <Bone className="h-7 w-24" />
                <Bone className="h-3 w-20" />
              </div>
            ))}
          </div>
          <Bone className="h-px w-full" />
          <div className="grid grid-cols-3 gap-4">
            {[1, 2, 3].map((i) => (
              <div key={i} className="space-y-2">
                <Bone className="h-3 w-20" />
                <Bone className="h-6 w-20" />
              </div>
            ))}
          </div>
        </div>
        <div className="rounded-lg border p-6 space-y-4">
          <Bone className="h-3 w-28" />
          <div className="flex justify-center">
            <Bone className="h-44 w-44 rounded-full" />
          </div>
          <div className="flex justify-center gap-4">
            {[1, 2, 3, 4].map((i) => <Bone key={i} className="h-3 w-16" />)}
          </div>
        </div>
      </div>
      {/* Breakdowns */}
      <div className="grid gap-4 md:grid-cols-3">
        {[1, 2, 3].map((i) => (
          <div key={i} className="rounded-lg border p-6 space-y-3">
            <Bone className="h-3 w-24" />
            {[1, 2, 3, 4].map((j) => (
              <div key={j} className="space-y-1.5">
                <div className="flex justify-between"><Bone className="h-3 w-20" /><Bone className="h-3 w-16" /></div>
                <Bone className="h-1.5 w-full" />
              </div>
            ))}
          </div>
        ))}
      </div>
    </div>
  )
}

/** Skeleton for a transaction table */
export function TableSkeleton({ rows = 25, cols = 5 }: { rows?: number; cols?: number }) {
  return (
    <div>
      <table className="w-full">
        <thead>
          <tr className="border-b">
            {Array.from({ length: cols }).map((_, i) => (
              <th key={i} className="px-4 py-3 text-left">
                <Bone className={cn("h-3", i === 0 ? "w-10" : i === 1 ? "w-16" : i === 2 ? "w-16" : "w-14")} />
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {Array.from({ length: rows }).map((_, i) => (
            <tr key={i} className="border-b">
              <td className="px-4 py-3"><Bone className="h-3 w-20" /></td>
              <td className="px-4 py-3"><Bone className={cn("h-3", i % 3 === 0 ? "w-24" : "w-20")} /></td>
              <td className="px-4 py-3"><Bone className="h-5 w-28 rounded-full" /></td>
              <td className="px-4 py-3 text-right"><Bone className="h-3 w-16 ml-auto" /></td>
              <td className="px-4 py-3"><Bone className={cn("h-3", i % 2 === 0 ? "w-24" : "w-20")} /></td>
            </tr>
          ))}
        </tbody>
      </table>
      {/* Pagination skeleton */}
      <div className="flex items-center justify-between border-t px-4 py-3">
        <Bone className="h-3 w-28" />
        <div className="flex items-center gap-2">
          <Bone className="h-3 w-20" />
          <Bone className="h-7 w-7 rounded-md" />
          <Bone className="h-7 w-7 rounded-md" />
        </div>
      </div>
    </div>
  )
}

/** Skeleton for a chart card */
export function ChartSkeleton({ height = 320 }: { height?: number }) {
  return (
    <div className="rounded-lg border p-4 space-y-3">
      <Bone className="h-3 w-32" />
      <Bone className={`w-full`} style={{ height }} />
      <div className="flex justify-center gap-4">
        {[1, 2, 3, 4].map((i) => <Bone key={i} className="h-3 w-16" />)}
      </div>
    </div>
  )
}

/** Skeleton for account cards grid */
export function AccountsGridSkeleton() {
  return (
    <div className="space-y-6">
      {[1, 2].map((section) => (
        <div key={section}>
          <Bone className="h-3 w-16 mb-3" />
          <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
            {[1, 2, 3].map((i) => (
              <div key={i} className="rounded-lg border p-5 space-y-3">
                <div className="flex justify-between">
                  <Bone className="h-4 w-28" />
                  <Bone className="h-5 w-16 rounded-full" />
                </div>
                <Bone className="h-3 w-16" />
                <Bone className="h-7 w-32" />
                <Bone className="h-3 w-24" />
              </div>
            ))}
          </div>
        </div>
      ))}
    </div>
  )
}

/** Skeleton for budget spreadsheet with section headers and cells */
export function SpreadsheetSkeleton() {
  const cols = 7
  const sections = [
    { rows: 1 },
    { rows: 4 },
    { rows: 6 },
    { rows: 2 },
  ]

  return (
    <div className="rounded-lg border overflow-hidden overflow-x-auto">
      <table className="w-full">
        <thead>
          <tr className="border-b">
            <th className="px-4 py-2 text-left w-[120px]"><Bone className="h-2.5 w-14" /></th>
            {Array.from({ length: cols }).map((_, i) => (
              <th key={i} className="px-3 py-2 text-right"><Bone className="h-2.5 w-8 ml-auto" /></th>
            ))}
            <th className="px-3 py-2 text-right"><Bone className="h-2.5 w-10 ml-auto" /></th>
            <th className="px-3 py-2 text-right"><Bone className="h-2.5 w-10 ml-auto" /></th>
          </tr>
        </thead>
        <tbody>
          {sections.map((section, si) => (
            <SectionSkeleton key={si} rows={section.rows} cols={cols} />
          ))}
        </tbody>
      </table>
    </div>
  )
}

function SectionSkeleton({ rows, cols }: { rows: number; cols: number }) {
  return (
    <>
      {/* Section header */}
      <tr className="bg-muted/50 border-b">
        <td colSpan={cols + 3} className="px-4 py-1.5">
          <Bone className="h-2.5 w-16" />
        </td>
      </tr>
      {/* Data rows */}
      {Array.from({ length: rows }).map((_, ri) => (
        <tr key={ri} className="border-b">
          <td className="px-4 py-2"><Bone className={cn("h-2.5", ri % 2 === 0 ? "w-16" : "w-20")} /></td>
          {Array.from({ length: cols }).map((_, ci) => (
            <td key={ci} className="px-3 py-2 text-right">
              <Bone className={cn("h-2.5 ml-auto", ci % 3 === 0 ? "w-12" : "w-10")} />
            </td>
          ))}
          <td className="px-3 py-2 text-right"><Bone className="h-2.5 w-10 ml-auto" /></td>
          <td className="px-3 py-2 text-right"><Bone className="h-2.5 w-8 ml-auto" /></td>
        </tr>
      ))}
      {/* Total row */}
      <tr className="border-b-2">
        <td className="px-4 py-2"><Bone className="h-2.5 w-14" /></td>
        {Array.from({ length: cols }).map((_, ci) => (
          <td key={ci} className="px-3 py-2 text-right"><Bone className="h-2.5 w-12 ml-auto" /></td>
        ))}
        <td className="px-3 py-2 text-right"><Bone className="h-2.5 w-12 ml-auto" /></td>
        <td className="px-3 py-2" />
      </tr>
    </>
  )
}

/** Skeleton for budget charts view */
export function BudgetChartsSkeleton() {
  return (
    <div className="space-y-4">
      <div className="grid gap-4 lg:grid-cols-2">
        <ChartSkeleton height={280} />
        <ChartSkeleton height={280} />
      </div>
      <ChartSkeleton height={300} />
    </div>
  )
}

/** Skeleton for portfolio history */
export function PortfolioHistorySkeleton() {
  return (
    <div className="space-y-6">
      <ChartSkeleton height={340} />
      <TableSkeleton rows={8} cols={5} />
    </div>
  )
}
