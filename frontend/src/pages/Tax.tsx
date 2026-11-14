import { useMemo, useState } from "react"
import { ukTaxYearForDate, ukTaxYearToDates } from "@/api/cgt_filter_params"
import type { IncomeCheck } from "@/bindings/IncomeCheck"
import type { IncomeRecord } from "@/bindings/IncomeRecord"
import type { IncomeSource } from "@/bindings/IncomeSource"
import {
  useIncomeChecks, useIncomeLineTypes, useIncomeRecords, useIncomeSources, useProfilesData, useTaxSummary,
} from "@/hooks/data"
import { useUrlFilters } from "@/hooks/use_url_filters"
import { usePreferredCurrency } from "@/context/preferred_currency_context"
import { Button } from "@/components/ui/button"
import { Select, SelectContent, SelectItem, SelectTrigger } from "@/components/ui/select"
import { EmptyState } from "@/components/empty_state"
import { Plus } from "lucide-react"
import { CheckDialog } from "./tax/check_dialog"
import { EmploymentCard } from "./tax/employment_card"
import { ExpenseMappingDialog } from "./tax/expense_mapping_dialog"
import { GiftAidCard, InterestCard } from "./tax/interest_gift_aid_cards"
import { ReadinessChecklist } from "./tax/readiness_checklist"
import { RecordDialog } from "./tax/record_dialog"
import { SelfEmploymentCard } from "./tax/self_employment_card"
import { SourceDialog } from "./tax/source_dialog"
import { SyncDialog } from "./tax/sync_dialog"
import { SectionError } from "./tax/tax_ui"

/** Year window subtitle; empty for a malformed year coming from the server. */
function yearWindow(taxYear: string): string {
  try {
    const [start, end] = ukTaxYearToDates(taxYear)
    return `${start} to ${end}`
  } catch {
    return ""
  }
}

type DialogState =
  | { kind: "source"; source: IncomeSource | null }
  | { kind: "record"; source: IncomeSource; record: IncomeRecord | null }
  | { kind: "check"; source: IncomeSource; check: IncomeCheck | undefined }
  | { kind: "mapping"; source: IncomeSource }
  | { kind: "sync"; source: IncomeSource }

export function TaxPage() {
  const currentTaxYear = useMemo(() => ukTaxYearForDate(new Date()), [])
  const [taxYear, setTaxYear] = useState(currentTaxYear)
  const preferred = usePreferredCurrency()

  const { profileId: activeProfileId } = useUrlFilters()
  const [profilesData] = useProfilesData()
  const profiles = profilesData.status === "succeeded" || profilesData.status === "reloading" ? profilesData.value : []
  const activeIsValid = !!activeProfileId && profiles.some((p) => p.id === activeProfileId)
  const [pickedProfile, setPickedProfile] = useState<string | null>(null)
  const profileId = activeIsValid ? activeProfileId : (pickedProfile ?? profiles[0]?.id ?? "")

  const [lineTypes, reloadLineTypes] = useIncomeLineTypes()
  const [sourcesData, reloadSources] = useIncomeSources(profileId)
  const [recordsData, reloadRecords] = useIncomeRecords({ profileId })
  const [checksData, reloadChecks] = useIncomeChecks(profileId, taxYear)
  const [summaryData, reloadSummary] = useTaxSummary(taxYear, profileId)
  const [dialog, setDialog] = useState<DialogState | null>(null)

  const catalogue = lineTypes.status === "succeeded" || lineTypes.status === "reloading" ? lineTypes.value : []
  const allRecords = recordsData.status === "succeeded" || recordsData.status === "reloading" ? recordsData.value : []
  const checks = checksData.status === "succeeded" || checksData.status === "reloading" ? checksData.value : []
  const summary = summaryData.status === "succeeded" || summaryData.status === "reloading" ? summaryData.value : undefined

  const yearOptions = useMemo(() => {
    const years = new Set<string>([currentTaxYear])
    for (const r of allRecords) years.add(r.tax_year)
    return [...years].sort((a, b) => b.localeCompare(a))
  }, [allRecords, currentTaxYear])

  function reloadAll() {
    reloadSources()
    reloadRecords()
    reloadChecks()
    reloadSummary()
  }

  function saved() {
    setDialog(null)
    reloadAll()
  }

  const windowLabel = yearWindow(taxYear)
  const yearRecords = (sourceId: string) =>
    allRecords.filter((r) => r.source_id === sourceId && r.tax_year === taxYear)
  const checkOf = (sourceId: string) => checks.find((c) => c.source_id === sourceId)

  const sources = sourcesData.status === "succeeded" || sourcesData.status === "reloading" ? sourcesData.value : []
  const employmentSources = sources.filter((s) => s.source_type === "employment")
  const businessSources = sources.filter((s) => s.source_type === "self_employment")

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center gap-3">
        <h1 className="text-lg font-semibold">Tax</h1>
        {!activeIsValid && profiles.length > 1 && (
          <Select value={profileId} onValueChange={(v) => v && setPickedProfile(v)}>
            <SelectTrigger className="w-[160px]" aria-label="Taxpayer profile">
              <span>{profiles.find((p) => p.id === profileId)?.name ?? "Profile"}</span>
            </SelectTrigger>
            <SelectContent>
              {profiles.map((p) => <SelectItem key={p.id} value={p.id}>{p.name}</SelectItem>)}
            </SelectContent>
          </Select>
        )}
        <Select value={taxYear} onValueChange={(v) => v && setTaxYear(v)}>
          <SelectTrigger className="w-[150px]">
            <span>{taxYear}</span>
          </SelectTrigger>
          <SelectContent>
            {yearOptions.map((y) => (
              <SelectItem key={y} value={y}>{y === currentTaxYear ? `${y} (current)` : y}</SelectItem>
            ))}
          </SelectContent>
        </Select>
        {windowLabel && <span className="text-sm text-muted-foreground">{windowLabel}</span>}
        <div className="flex-1" />
        <Button size="sm" onClick={() => setDialog({ kind: "source", source: null })} disabled={!profileId}>
          <Plus className="mr-1.5 h-4 w-4" />
          Add income source
        </Button>
      </div>

      <ReadinessChecklist data={summaryData} taxYear={taxYear} onRetry={reloadSummary} />

      {sourcesData.status === "failed" && <SectionError error={sourcesData.error} onRetry={reloadSources} />}
      {lineTypes.status === "failed" && <SectionError error={lineTypes.error} onRetry={reloadLineTypes} />}

      {sourcesData.status === "succeeded" && sources.length === 0 && (
        <EmptyState
          title="No income sources yet"
          message="Add an employer or a business for this taxpayer, then record payslips and sales against it."
          action={{ label: "Add income source", onClick: () => setDialog({ kind: "source", source: null }) }}
        />
      )}

      {employmentSources.length > 0 && (
        <section className="space-y-3">
          <h2 className="text-sm font-semibold text-muted-foreground">Employment</h2>
          {employmentSources.map((s) => (
            <EmploymentCard
              key={s.id}
              source={s}
              summary={summary?.employment.find((e) => e.source_id === s.id)}
              records={yearRecords(s.id)}
              check={checkOf(s.id)}
              catalogue={catalogue}
              taxYear={taxYear}
              currency={preferred}
              onEditSource={() => setDialog({ kind: "source", source: s })}
              onAddRecord={() => setDialog({ kind: "record", source: s, record: null })}
              onEditRecord={(record) => setDialog({ kind: "record", source: s, record })}
              onEditCheck={(check) => setDialog({ kind: "check", source: s, check })}
              reload={reloadAll}
            />
          ))}
        </section>
      )}

      {businessSources.length > 0 && (
        <section className="space-y-3">
          <h2 className="text-sm font-semibold text-muted-foreground">Self-employment</h2>
          {businessSources.map((s) => (
            <SelfEmploymentCard
              key={s.id}
              source={s}
              summary={summary?.self_employment.find((e) => e.source_id === s.id)}
              records={yearRecords(s.id)}
              check={checkOf(s.id)}
              catalogue={catalogue}
              taxYear={taxYear}
              currency={preferred}
              onEditSource={() => setDialog({ kind: "source", source: s })}
              onAddRecord={() => setDialog({ kind: "record", source: s, record: null })}
              onEditRecord={(record) => setDialog({ kind: "record", source: s, record })}
              onEditCheck={(check) => setDialog({ kind: "check", source: s, check })}
              onSync={() => setDialog({ kind: "sync", source: s })}
              onMapCategories={() => setDialog({ kind: "mapping", source: s })}
              reload={reloadAll}
            />
          ))}
        </section>
      )}

      <div className="grid gap-4 lg:grid-cols-2">
        <InterestCard data={summaryData} taxYear={taxYear} onRetry={reloadSummary} />
        <GiftAidCard data={summaryData} taxYear={taxYear} onRetry={reloadSummary} />
      </div>

      {dialog?.kind === "source" && (
        <SourceDialog profileId={profileId} source={dialog.source} onClose={() => setDialog(null)} onSaved={saved} />
      )}
      {dialog?.kind === "record" && (
        <RecordDialog
          source={dialog.source}
          record={dialog.record}
          taxYear={taxYear}
          catalogue={catalogue}
          onClose={() => setDialog(null)}
          onSaved={saved}
        />
      )}
      {dialog?.kind === "check" && (
        <CheckDialog
          source={dialog.source}
          check={dialog.check}
          taxYear={taxYear}
          onClose={() => setDialog(null)}
          onSaved={saved}
        />
      )}
      {dialog?.kind === "mapping" && (
        <ExpenseMappingDialog
          source={dialog.source}
          unmapped={summary?.self_employment.find((e) => e.source_id === dialog.source.id)?.bank_expenses.unmapped ?? []}
          catalogue={catalogue}
          currency={preferred}
          onClose={() => setDialog(null)}
          onSaved={saved}
        />
      )}
      {dialog?.kind === "sync" && (
        <SyncDialog
          source={dialog.source}
          taxYear={taxYear}
          currency={preferred}
          onClose={() => setDialog(null)}
          onSynced={reloadAll}
        />
      )}
    </div>
  )
}
