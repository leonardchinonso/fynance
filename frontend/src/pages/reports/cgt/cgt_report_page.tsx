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
import { api } from "@/api/client"
import { useCapitalGains } from "@/hooks/data/use_capital_gains"
import { CgtFilterBar } from "./cgt_filter_bar"
import { taxInputsPayloadForBand, type CgtBandSelection } from "./band_headroom"
import { CgtSummaryCard } from "./cgt_summary_card"
import { CgtPerSymbolTable } from "./cgt_per_symbol_table"
import { CgtDisposalSchedule } from "./cgt_disposal_schedule"
import { CgtPoolWorkings } from "./cgt_pool_workings"
import { CgtHistoryList } from "./cgt_history_list"
import { CgtPdfDocument } from "./cgt_pdf_document"
import { CgtPreflight } from "./cgt_preflight"
import type { MissingRatePair } from "@/bindings/MissingRatePair"
import {
  deleteStoredReport,
  getStoredReport,
  listStoredReports,
  newReportId,
  saveStoredReport,
  type StoredCgtReport,
} from "./stored_reports"

/**
 * Extract the missing-rate list from a failed generate, or `null` if this wasn't that error.
 *
 * Reads the structured `missing`/`quote` fields the backend sends alongside `code`, rather than
 * parsing the human-readable message — the message is prose for a person, the array is the form
 * the user fills in. Shape-checks defensively so a malformed body falls back to the generic
 * error card instead of rendering an empty pre-flight the user cannot get past.
 */
function missingRatesFrom(
  err: unknown,
): { missing: MissingRatePair[]; quote: string } | null {
  if (!(err instanceof ApiError) || err.code !== "missing_exchange_rates") return null
  const raw = err.body?.missing
  if (!Array.isArray(raw) || raw.length === 0) return null
  const missing: MissingRatePair[] = []
  for (const item of raw) {
    if (
      item &&
      typeof item === "object" &&
      typeof (item as MissingRatePair).currency === "string" &&
      typeof (item as MissingRatePair).date === "string"
    ) {
      missing.push(item as MissingRatePair)
    }
  }
  if (missing.length === 0) return null
  const quote = typeof err.body?.quote === "string" ? err.body.quote : "GBP"
  return { missing, quote }
}

export function CgtReportPage() {
  const { reportId } = useParams<{ reportId?: string }>()
  const navigate = useNavigate()
  const { profilesData } = useProfiles()
  const profiles = profilesData.status === "succeeded" ? profilesData.value : []
  const { profileId: activeProfileId } = useUrlFilters()
  const { state, error: generateError, generate } = useCapitalGains()
  const [reports, setReports] = useState<StoredCgtReport[]>(() => listStoredReports())
  // Set when the backend reports `missing_exchange_rates`. Holds the pairs to collect plus the
  // filters that triggered it, so generation can be retried unchanged once they are saved.
  const [preflight, setPreflight] = useState<{
    missing: MissingRatePair[]
    quote: string
    filters: CgtFilters
    band: CgtBandSelection | null
  } | null>(null)

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

  /**
   * Run the report, or divert to the pre-flight screen when the backend says rates are missing.
   *
   * `confirmedUtr` is supplied on the retry that follows pre-flight; on a first attempt the
   * profile's stored UTR is used. It is snapshotted onto the stored report so reprinting an old
   * report reproduces what was filed rather than picking up a later profile edit.
   */
  async function handleGenerate(
    filters: CgtFilters,
    band: CgtBandSelection | null,
    confirmedUtr?: string | null,
  ) {
    try {
      // The band selector is an input to the computation, not a display flag,
      // so it has to reach the server before the report runs. It is stored as
      // `allowable_income_remaining`: "basic" means enough unused basic-rate
      // income band to cover the whole gain, "higher" means none. Only the
      // headroom is written — brought-forward losses and the AEA choice are the
      // user's own settings and must not be disturbed by generating a report.
      //
      // An untouched selector yields no payload at all, so generating a report
      // leaves the stored headroom exactly as the user set it. That decision
      // lives entirely in `taxInputsPayloadForBand`, which is what this branch
      // tests — there is no second condition here to drift away from it.
      const payload = taxInputsPayloadForBand(band)
      if (payload && filters.period.kind === "tax-year" && filters.profileId) {
        await api.putTaxInputs(filters.profileId, filters.period.taxYear, payload)
      }
      const response = await generate(filters)
      const id = newReportId()
      const utr =
        confirmedUtr !== undefined
          ? confirmedUtr
          : (profiles.find((p) => p.id === filters.profileId)?.utr ?? null)
      const report: StoredCgtReport = {
        id,
        generatedAt: new Date().toISOString(),
        filters,
        utr,
        response,
      }
      saveStoredReport(report)
      setReports(listStoredReports())
      setPreflight(null)
      navigate(`/reports/cgt/${id}`)
    } catch (err) {
      const missing = missingRatesFrom(err)
      if (missing) {
        setPreflight({ ...missing, filters, band })
        return
      }
      // Anything else stays a plain failure; `state`/`generateError` render it below.
    }
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

      {preflight && (
        <CgtPreflight
          missing={preflight.missing}
          quote={preflight.quote}
          profile={profiles.find((p) => p.id === preflight.filters.profileId)}
          taxYear={
            preflight.filters.period.kind === "tax-year"
              ? preflight.filters.period.taxYear
              : null
          }
          onCancel={() => setPreflight(null)}
          onReady={(confirmedUtr) =>
            handleGenerate(preflight.filters, preflight.band, confirmedUtr)
          }
        />
      )}

      {/* The pre-flight screen replaces the error card for missing rates: a rate that hasn't
          been entered yet is a step in the workflow, not a failure to report. */}
      {!preflight && state.status === "failed" && <GenerateError error={generateError} />}

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
        tax={response.tax}
      />

      <CgtPerSymbolTable
        rows={response.symbol_summaries}
        baseCurrency={response.summary.base_currency}
        perSymbolCounts={perSymbolCounts}
      />

      <CgtDisposalSchedule rows={response.realized_events} />

      <CgtPoolWorkings pools={response.pools} />

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
