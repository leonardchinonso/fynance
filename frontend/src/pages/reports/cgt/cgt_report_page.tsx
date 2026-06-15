import { useEffect, useMemo, useState } from "react"
import { useNavigate, useParams } from "react-router-dom"
import { PDFDownloadLink } from "@react-pdf/renderer"
import { AlertTriangle, Receipt } from "lucide-react"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Button } from "@/components/ui/button"
import { ApiError } from "@/api/real_service"
import { useProfiles } from "@/context/profile_context"
import { useUrlFilters } from "@/hooks/use_url_filters"
import type { CgtFilters } from "@/api/service"
import { previousUkTaxYearForDate, periodSlug } from "@/api/cgt_filter_params"
import { useCapitalGains } from "@/hooks/data/use_capital_gains"
import { CgtFilterBar } from "./cgt_filter_bar"
import { CgtSummaryCard } from "./cgt_summary_card"
import { CgtPerSymbolTable } from "./cgt_per_symbol_table"
import { CgtDisposalSchedule } from "./cgt_disposal_schedule"
import { CgtPoolWorkings } from "./cgt_pool_workings"
import { CgtHistoryList } from "./cgt_history_list"
import { CgtPdfDocument } from "./cgt_pdf_document"
import {
  deleteStoredReport,
  getStoredReport,
  listStoredReports,
  newReportId,
  saveStoredReport,
  type StoredCgtReport,
} from "./stored_reports"

export function CgtReportPage() {
  const { reportId } = useParams<{ reportId?: string }>()
  const navigate = useNavigate()
  const { profilesData } = useProfiles()
  const profiles = profilesData.status === "succeeded" ? profilesData.value : []
  const { profileId: activeProfileId } = useUrlFilters()
  const { state, error: generateError, generate } = useCapitalGains()
  const [reports, setReports] = useState<StoredCgtReport[]>(() => listStoredReports())

  const defaultFilters = useMemo<CgtFilters>(() => {
    const preselected =
      activeProfileId && profiles.some((p) => p.id === activeProfileId)
        ? activeProfileId
        : (profiles[0]?.id ?? "")
    return {
      period: { kind: "tax-year", taxYear: previousUkTaxYearForDate(new Date()) },
      profileId: preselected,
    }
  }, [activeProfileId, profiles])

  const stored = reportId ? getStoredReport(reportId) : undefined

  // If the URL has a reportId we don't recognise, fall back to the filter view
  // and let the user generate a fresh one.
  useEffect(() => {
    if (reportId && !stored) {
      // No-op for now; UI shows the not-found state below.
    }
  }, [reportId, stored])

  async function handleGenerate(filters: CgtFilters) {
    const response = await generate(filters)
    const id = newReportId()
    const report: StoredCgtReport = {
      id,
      generatedAt: new Date().toISOString(),
      filters,
      response,
    }
    saveStoredReport(report)
    setReports(listStoredReports())
    navigate(`/reports/cgt/${id}`)
  }

  function handleDelete(id: string) {
    deleteStoredReport(id)
    setReports(listStoredReports())
    if (reportId === id) navigate("/reports/cgt")
  }

  if (reportId && stored) {
    return <SavedReportView report={stored} onBack={() => navigate("/reports/cgt")} />
  }

  return (
    <div className="space-y-4 py-4">
      <div className="flex items-center gap-3">
        <div className="flex h-10 w-10 items-center justify-center rounded-lg bg-secondary">
          <Receipt className="h-5 w-5" />
        </div>
        <div>
          <h1 className="text-xl font-semibold">Capital Gains Tax report</h1>
          <p className="text-sm text-muted-foreground">
            UK HMRC-style report of your disposals, gains, and S104 pool workings.
          </p>
        </div>
      </div>

      <Card>
        <CardHeader>
          <CardTitle>Filters</CardTitle>
        </CardHeader>
        <CardContent>
          {profilesData.status === "succeeded" ? (
            <CgtFilterBar
              profiles={profiles}
              initial={defaultFilters}
              loading={state.status === "loading" || state.status === "reloading"}
              onGenerate={handleGenerate}
            />
          ) : (
            <p className="text-sm text-muted-foreground">Loading profiles…</p>
          )}
        </CardContent>
      </Card>

      {reportId && !stored && (
        <Card>
          <CardContent className="py-6">
            <p className="text-sm">
              This report isn't saved on this device. Pick filters above and press
              Generate to create a fresh one.
            </p>
          </CardContent>
        </Card>
      )}

      {state.status === "failed" && <GenerateError error={generateError} />}

      <CgtHistoryList reports={reports} onDelete={handleDelete} />
    </div>
  )
}

function GenerateError({ error }: { error: Error | null }) {
  const navigate = useNavigate()
  const apiError = error instanceof ApiError ? error : null
  const isMissingCurrencies = apiError?.code === "missing_currencies"

  return (
    <Card className="border-amber-400/50 bg-amber-50 dark:bg-amber-950/30">
      <CardContent className="py-5">
        <div className="flex items-start gap-3">
          <AlertTriangle className="mt-0.5 h-5 w-5 shrink-0 text-amber-600 dark:text-amber-400" />
          <div className="flex-1 space-y-2">
            <h3 className="text-sm font-semibold">
              {isMissingCurrencies
                ? "Configure currencies before generating"
                : "Failed to generate report"}
            </h3>
            <p className="text-sm text-muted-foreground">
              {error?.message ?? "Unknown error"}
            </p>
            {isMissingCurrencies && (
              <Button
                size="sm"
                onClick={() => navigate("/settings/general")}
              >
                Go to Settings → Currencies
              </Button>
            )}
          </div>
        </div>
      </CardContent>
    </Card>
  )
}

function SavedReportView({
  report,
  onBack,
}: {
  report: StoredCgtReport
  onBack: () => void
}) {
  const { filters, response } = report

  const perSymbolCounts = useMemo(() => {
    const counts: Record<string, number> = {}
    for (const ev of response.realized_events) {
      counts[ev.symbol] = (counts[ev.symbol] ?? 0) + 1
    }
    return counts
  }, [response.realized_events])

  const symbolCurrencies = useMemo(() => {
    const m: Record<string, string> = {}
    for (const ev of response.realized_events) {
      m[ev.symbol] = ev.original_currency
    }
    return m
  }, [response.realized_events])

  const filename = `fynance-cgt-${periodSlug(filters.period)}-${report.generatedAt.slice(0, 10)}.pdf`

  return (
    <div className="space-y-4 py-4">
      <div className="flex items-center justify-between gap-3">
        <div className="flex items-center gap-3">
          <Button variant="outline" size="sm" onClick={onBack}>
            ← All reports
          </Button>
          <div>
            <h1 className="text-xl font-semibold">Capital Gains Tax report</h1>
            <p className="text-sm text-muted-foreground">
              Generated {new Date(report.generatedAt).toLocaleString()}
            </p>
          </div>
        </div>
      </div>

      <CgtSummaryCard
        summary={response.summary}
        disposalCount={response.realized_events.length}
      />

      <CgtPerSymbolTable
        rows={response.symbol_summaries}
        baseCurrency={response.summary.base_currency}
        perSymbolCounts={perSymbolCounts}
      />

      <CgtDisposalSchedule rows={response.realized_events} />

      <CgtPoolWorkings
        pools={response.pools}
        symbolCurrencies={symbolCurrencies}
        defaultCurrency={response.summary.base_currency}
      />

      <div className="sticky bottom-4 flex justify-end">
        <PDFDownloadLink
          document={<CgtPdfDocument report={report} />}
          fileName={filename}
        >
          {({ loading }) => (
            <Button
              disabled={loading}
              className="cursor-pointer disabled:cursor-not-allowed"
            >
              {loading ? "Preparing PDF…" : "Generate PDF"}
            </Button>
          )}
        </PDFDownloadLink>
      </div>
    </div>
  )
}
