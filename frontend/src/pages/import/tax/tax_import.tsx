import { useEffect, useState } from "react"
import { useNavigate, useSearchParams } from "react-router-dom"
import { AlertTriangle, ArrowLeft, CheckCircle2 } from "lucide-react"
import { api } from "@/api/client"
import type { Profile } from "@/types"
import type { ParseHints } from "@/bindings/ParseHints"
import type { TaxDocumentsImportResult } from "@/bindings/TaxDocumentsImportResult"
import { Button } from "@/components/ui/button"
import { Card, CardContent } from "@/components/ui/card"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { ReloadingOverlay } from "@/components/reloading_overlay"
import { extractErrorMessage } from "@/lib/error_message"
import { useRecentImports } from "@/hooks/use_recent_imports"
import { FileUpload } from "../file_upload"
import { RecentImportsList } from "../recent_imports_list"
import { ParseProgressController, type ParseProgressUi } from "../parse_progress"
import { TaxPreviewReview } from "./tax_preview_review"

const TAX_HINTS_BASE: Omit<ParseHints, "hint"> = {
  return_type: {
    transactions: false,
    holdings: { enabled: false, period: null },
    investments: false,
    tax_documents: true,
  },
}

/**
 * The tax-document import flow, derived from the URL like the statement flow:
 * `/import/tax` is setup, `?entry=<id>` is the preview, `?done=1` the summary.
 */
export function TaxImport() {
  const navigate = useNavigate()
  const [searchParams] = useSearchParams()
  const entryParam = searchParams.get("entry")
  const doneParam = searchParams.get("done") === "1"
  // The navbar profile; also the profile the done screen links back to.
  const profileParam = searchParams.get("profile")

  const [profiles, setProfiles] = useState<Profile[] | null>(null)
  const [profileId, setProfileId] = useState<string | null>(null)
  const [files, setFiles] = useState<File[]>([])
  const [hint, setHint] = useState<string | null>(null)
  const [parsing, setParsing] = useState(false)
  const [progress, setProgress] = useState<ParseProgressUi | null>(null)
  const [parseError, setParseError] = useState<string | null>(null)
  const [outcome, setOutcome] = useState<TaxDocumentsImportResult | null>(null)
  const recents = useRecentImports()

  useEffect(() => {
    api
      .getProfiles()
      .then((all) => {
        setProfiles(all)
        // One profile: pre-fill it. Several: pre-select the navbar profile when it is one of them, else ask.
        if (all.length === 1) setProfileId(all[0].id)
        else if (profileParam && all.some((p) => p.id === profileParam)) setProfileId(profileParam)
      })
      .catch(() => setProfiles([]))
  }, [])

  const entry = entryParam ? (recents.entries.find((e) => e.id === entryParam) ?? null) : null
  const taxResult = entry?.kind === "tax" ? (entry.preview.tax_documents ?? null) : null
  const step: "setup" | "preview" | "done" =
    doneParam ? "done" : entry && taxResult ? "preview" : "setup"

  const options = profiles ?? []
  const selected = options.find((p) => p.id === profileId) ?? null

  async function handleSubmit() {
    if (!selected || files.length === 0) return
    setParsing(true)
    setParseError(null)
    const ctrl = new ParseProgressController(files)
    setProgress(ctrl.sample())
    const ticker = window.setInterval(() => setProgress(ctrl.sample()), 120)
    try {
      const result = await api.parseTaxDocuments(files, selected.id, hint, {
        onProgress: (e) => ctrl.onEvent(e),
      })
      ctrl.complete()
      setProgress(ctrl.sample())
      const id = recents.add({
        accountId: "",
        kind: "tax",
        profileId: selected.id,
        fileNames: files.map((f) => f.name),
        preview: result,
      })
      navigate(`/import/tax?entry=${id}`)
    } catch (err) {
      console.error("Tax document parse failed:", err)
      setParseError(extractErrorMessage(err))
    } finally {
      window.clearInterval(ticker)
      setParsing(false)
      setProgress(null)
    }
  }

  function handleBack() {
    setParseError(null)
    if (step === "preview") navigate("/import/tax")
    else navigate("/import")
  }

  const taxEntries = recents.entries.filter((e) => e.kind === "tax")
  const containerWidth = step === "preview" ? "max-w-[88rem]" : "max-w-3xl"

  return (
    <div className={`${containerWidth} mx-auto py-4`}>
      <div className="mb-6 flex items-center gap-3">
        <Button variant="ghost" size="icon" className="h-8 w-8" onClick={handleBack} aria-label="Back">
          <ArrowLeft className="h-4 w-4" />
        </Button>
        <div>
          <h1 className="text-xl font-semibold">Import tax documents</h1>
          <p className="text-sm text-muted-foreground">
            Payslips and P60s, PDF or photo. Review, then save to your tax records.
          </p>
        </div>
      </div>

      <Card>
        <CardContent className="pt-6">
          {step === "setup" && (
            <div className="space-y-4">
              {profiles === null ? (
                <p className="text-sm text-muted-foreground">Loading profiles...</p>
              ) : options.length === 0 ? (
                <div className="space-y-3 text-center">
                  <p className="text-sm text-muted-foreground">
                    No profiles found. Create a profile in Settings first.
                  </p>
                  <Button variant="outline" onClick={() => navigate("/settings")}>
                    Go to Settings
                  </Button>
                </div>
              ) : (
                <>
                  <div className="space-y-1.5">
                    <p className="text-sm font-medium">Whose documents are these?</p>
                    <Select
                      value={profileId ?? ""}
                      onValueChange={(v) => setProfileId(v)}
                      items={Object.fromEntries(options.map((p) => [p.id, p.name]))}
                      disabled={parsing}
                    >
                      <SelectTrigger className="w-full" aria-label="Profile">
                        <SelectValue placeholder="Choose a profile" />
                      </SelectTrigger>
                      <SelectContent>
                        {options.map((p) => (
                          <SelectItem key={p.id} value={p.id}>
                            {p.name}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </div>
                  {selected ? (
                    <FileUpload
                      variant="tax"
                      files={files}
                      onFilesChange={setFiles}
                      hints={{ ...TAX_HINTS_BASE, hint }}
                      onHintsChange={(h) => setHint(h.hint)}
                      onSubmit={handleSubmit}
                      submitting={parsing}
                      accountName={selected.name}
                      accountInstitution="Tax documents"
                    />
                  ) : (
                    <p className="text-xs text-muted-foreground">Choose a profile to continue.</p>
                  )}
                  {parseError && (
                    <div className="flex items-start gap-2 rounded-lg border border-destructive/40 bg-destructive/5 p-3 text-sm">
                      <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0 text-destructive" />
                      <span className="text-xs text-destructive">{parseError}</span>
                    </div>
                  )}
                </>
              )}
            </div>
          )}

          {step === "preview" && entry && taxResult && (
            <TaxPreviewReview
              preview={entry.preview}
              result={taxResult}
              fileCount={entry.fileNames.length}
              initialEdits={entry.taxEdits}
              onEditsChanged={(edits) => recents.updateTaxEdits(entry.id, edits)}
              onCommitted={(res) => {
                setOutcome(res)
                navigate(`/import/tax?done=1&profile=${encodeURIComponent(taxResult.profile_id)}`)
              }}
              onCancel={() => navigate("/import/tax")}
            />
          )}

          {step === "done" && (
            <div className="space-y-4 text-center">
              <CheckCircle2 className="mx-auto h-10 w-10 text-green-600" />
              <div>
                <h2 className="text-lg font-semibold">Tax records saved</h2>
                {outcome ? (
                  <p className="text-sm text-muted-foreground">
                    {outcome.inserted} saved
                    {outcome.duplicates > 0 ? `, ${outcome.duplicates} duplicate${outcome.duplicates !== 1 ? "s" : ""} skipped` : ""}.
                  </p>
                ) : (
                  <p className="text-sm text-muted-foreground">Your records were saved.</p>
                )}
              </div>
              <div className="flex justify-center gap-2">
                <Button variant="outline" onClick={() => navigate("/import/tax")}>
                  Import more
                </Button>
                <Button
                  className="bg-blue-600 text-white hover:bg-blue-600/90"
                  onClick={() => navigate(profileParam ? `/tax?profile=${encodeURIComponent(profileParam)}` : "/tax")}
                >
                  View tax records
                </Button>
              </div>
            </div>
          )}
        </CardContent>
      </Card>

      {step === "setup" && (
        <RecentImportsList
          entries={taxEntries}
          accounts={[]}
          profiles={profiles ?? []}
          onResume={(e) => navigate(`/import/tax?entry=${e.id}`)}
          onDiscard={(id) => recents.remove(id)}
        />
      )}

      <ReloadingOverlay
        active={parsing}
        fullscreen
        progress={progress ?? undefined}
        text="Currently importing data. Please wait, this might take a while."
      />
    </div>
  )
}
