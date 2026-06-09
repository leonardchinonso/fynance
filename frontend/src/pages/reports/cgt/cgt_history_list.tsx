import { useNavigate } from "react-router-dom"
import { Trash2 } from "lucide-react"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { formatCurrency } from "@/lib/utils"
import { periodLabel } from "@/api/cgt_filter_params"
import { useProfiles } from "@/context/profile_context"
import type { StoredCgtReport } from "./stored_reports"

interface CgtHistoryListProps {
  reports: StoredCgtReport[]
  onDelete: (id: string) => void
}

export function CgtHistoryList({ reports, onDelete }: CgtHistoryListProps) {
  const navigate = useNavigate()
  const { profilesData } = useProfiles()
  const profiles = profilesData.status === "succeeded" ? profilesData.value : []
  if (reports.length === 0) {
    return (
      <Card>
        <CardHeader>
          <CardTitle>Recent reports</CardTitle>
        </CardHeader>
        <CardContent>
          <p className="text-sm text-muted-foreground">
            No reports yet. Pick a tax year above and press Generate to create one.
          </p>
        </CardContent>
      </Card>
    )
  }
  return (
    <Card>
      <CardHeader>
        <CardTitle>Recent reports</CardTitle>
      </CardHeader>
      <CardContent className="space-y-2">
        {reports.map((r) => {
          const net = Number.parseFloat(r.response.summary.net_gain_loss)
          const gainColour =
            net >= 0
              ? "text-emerald-600 dark:text-emerald-400"
              : "text-red-600 dark:text-red-400"
          const open = () => navigate(`/reports/cgt/${r.id}`)
          return (
            <div
              key={r.id}
              role="button"
              tabIndex={0}
              onClick={open}
              onKeyDown={(ev) => {
                if (ev.key === "Enter" || ev.key === " ") {
                  ev.preventDefault()
                  open()
                }
              }}
              className="w-full text-left rounded-md border bg-card p-3 hover:border-foreground/30 hover:shadow-sm transition-all cursor-pointer"
            >
              <div className="flex items-start justify-between gap-3">
                <div className="min-w-0 flex-1">
                  <div className="flex flex-wrap items-baseline gap-x-2 gap-y-1">
                    <span className="font-medium">{periodLabel(r.filters.period)}</span>
                    <Badge variant="secondary">
                      {profiles.find((p) => p.id === r.filters.profileId)?.name ?? r.filters.profileId}
                    </Badge>
                  </div>
                  <p className="mt-1 text-xs text-muted-foreground">
                    Generated {new Date(r.generatedAt).toLocaleString()}
                  </p>
                </div>
                <div className="text-right">
                  <p className={`tabular-nums font-medium ${gainColour}`}>
                    {formatCurrency(r.response.summary.net_gain_loss, r.response.summary.base_currency)}
                  </p>
                  <p className="text-xs text-muted-foreground">net gain/loss</p>
                </div>
                <Button
                  variant="ghost"
                  size="icon"
                  onClick={(ev) => {
                    ev.stopPropagation()
                    onDelete(r.id)
                  }}
                  aria-label="Delete saved report"
                >
                  <Trash2 className="h-4 w-4" />
                </Button>
              </div>
            </div>
          )
        })}
      </CardContent>
    </Card>
  )
}
