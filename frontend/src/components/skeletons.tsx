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

/** Skeleton for a table with rows */
export function TableSkeleton({ rows = 10, cols = 5 }: { rows?: number; cols?: number }) {
  return (
    <div className="space-y-4">
      {/* Filters placeholder */}
      <div className="flex gap-3">
        <Bone className="h-8 w-[180px]" />
        <Bone className="h-8 w-[200px]" />
        <Bone className="h-8 w-20" />
      </div>
      {/* Table */}
      <div className="rounded-lg border">
        {/* Header */}
        <div className="flex gap-4 border-b px-4 py-3">
          {Array.from({ length: cols }).map((_, i) => (
            <Bone key={i} className="h-3 flex-1" />
          ))}
        </div>
        {/* Rows */}
        {Array.from({ length: rows }).map((_, i) => (
          <div key={i} className="flex gap-4 border-b px-4 py-3 last:border-0">
            {Array.from({ length: cols }).map((_, j) => (
              <Bone key={j} className={cn("h-4 flex-1", j === 0 && "max-w-[100px]")} />
            ))}
          </div>
        ))}
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

/** Skeleton for budget spreadsheet */
export function SpreadsheetSkeleton() {
  return (
    <div className="rounded-lg border overflow-hidden">
      <div className="flex gap-2 border-b px-3 py-2">
        {Array.from({ length: 8 }).map((_, i) => (
          <Bone key={i} className="h-3 flex-1" />
        ))}
      </div>
      {Array.from({ length: 12 }).map((_, i) => (
        <div key={i} className="flex gap-2 border-b px-3 py-2.5 last:border-0">
          {Array.from({ length: 8 }).map((_, j) => (
            <Bone key={j} className={cn("h-4 flex-1", j === 0 && "max-w-[120px]")} />
          ))}
        </div>
      ))}
    </div>
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
