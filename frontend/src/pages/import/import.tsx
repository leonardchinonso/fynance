import { useState, useEffect } from "react"
import { useSearchParams, useNavigate } from "react-router-dom"
import { api } from "@/api/client"
import type { Account, ImportResult } from "@/types"
import { useIngestionPreferences } from "@/hooks/use_ingestion_preferences"
import { Card, CardContent } from "@/components/ui/card"
import { Button } from "@/components/ui/button"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { FileUpload } from "./file_upload"
import { PreviewTable } from "./preview_table"
import { ImportSummary } from "./import_summary"
import { WizardProgress } from "./wizard_progress"
import { ArrowLeft } from "lucide-react"

type Step = "account-select" | "upload" | "result" | "complete"

interface AccountResult {
  accountId: string
  accountName: string
  results: ImportResult[]
  skipped: boolean
}

export function ImportPage() {
  const [searchParams] = useSearchParams()
  const navigate = useNavigate()
  const mode = searchParams.get("mode") === "wizard" ? "wizard" : "single"

  const [allAccounts, setAllAccounts] = useState<Account[]>([])
  const [loading, setLoading] = useState(true)
  const { getOrderedAccounts } = useIngestionPreferences()

  // Wizard state
  const [step, setStep] = useState<Step>(mode === "wizard" ? "upload" : "account-select")
  const [selectedAccountId, setSelectedAccountId] = useState<string | null>(null)
  const [currentIndex, setCurrentIndex] = useState(0)
  const [files, setFiles] = useState<File[]>([])
  const [lastResult, setLastResult] = useState<ImportResult | null>(null)
  const [submitting, setSubmitting] = useState(false)
  const [accountResults, setAccountResults] = useState<AccountResult[]>([])
  const [completedIds, setCompletedIds] = useState<Set<string>>(new Set())
  const [skippedIds, setSkippedIds] = useState<Set<string>>(new Set())

  useEffect(() => {
    api.getAccounts().then((data) => {
      setAllAccounts(data)
      setLoading(false)
    }).catch(() => setLoading(false))
  }, [])

  const wizardAccounts = getOrderedAccounts(allAccounts)
  const currentAccount = mode === "wizard"
    ? wizardAccounts[currentIndex]
    : allAccounts.find((a) => a.id === selectedAccountId) ?? null

  async function handleSubmit() {
    if (!currentAccount || files.length === 0) return
    setSubmitting(true)
    try {
      const results: ImportResult[] = []
      for (const file of files) {
        const result = await api.importCsv(currentAccount.id, file)
        results.push(result)
      }
      const lastRes = results[results.length - 1]
      setLastResult(lastRes)
      setAccountResults((prev) => [...prev, {
        accountId: currentAccount.id,
        accountName: currentAccount.name,
        results,
        skipped: false,
      }])
      setCompletedIds((prev) => new Set(prev).add(currentAccount.id))
      setStep("result")
    } catch (err) {
      console.error("Import failed:", err)
      // Show a synthetic error result
      setLastResult({
        rows_total: BigInt(0),
        rows_inserted: BigInt(0),
        rows_duplicate: BigInt(0),
        filename: files[0]?.name ?? "unknown",
        account_id: currentAccount.id,
        detected_bank: "unknown",
        detection_confidence: 0,
        errors: [{ index: 0, reason: String(err) }],
      })
      setStep("result")
    } finally {
      setSubmitting(false)
    }
  }

  function handleSkip() {
    if (!currentAccount) return
    setSkippedIds((prev) => new Set(prev).add(currentAccount.id))
    setAccountResults((prev) => [...prev, {
      accountId: currentAccount.id,
      accountName: currentAccount.name,
      results: [],
      skipped: true,
    }])
    advanceToNext()
  }

  function advanceToNext() {
    setFiles([])
    setLastResult(null)
    if (mode === "wizard") {
      const nextIndex = currentIndex + 1
      if (nextIndex >= wizardAccounts.length) {
        setStep("complete")
      } else {
        setCurrentIndex(nextIndex)
        setStep("upload")
      }
    } else {
      setStep("complete")
    }
  }

  function handleResultDone() {
    advanceToNext()
  }

  function handleReset() {
    setStep(mode === "wizard" ? "upload" : "account-select")
    setCurrentIndex(0)
    setSelectedAccountId(null)
    setFiles([])
    setLastResult(null)
    setAccountResults([])
    setCompletedIds(new Set())
    setSkippedIds(new Set())
  }

  if (loading) {
    return (
      <div className="max-w-2xl mx-auto py-8">
        <p className="text-sm text-muted-foreground text-center">Loading accounts...</p>
      </div>
    )
  }

  if (allAccounts.length === 0) {
    return (
      <div className="max-w-2xl mx-auto py-8 text-center space-y-4">
        <p className="text-sm text-muted-foreground">No accounts found. Create an account in Settings first.</p>
        <Button variant="outline" onClick={() => navigate("/settings")}>Go to Settings</Button>
      </div>
    )
  }

  return (
    <div className="max-w-3xl mx-auto py-4">
      {/* Header */}
      <div className="flex items-center gap-3 mb-6">
        <Button variant="ghost" size="icon" className="h-8 w-8" onClick={() => navigate(-1)}>
          <ArrowLeft className="h-4 w-4" />
        </Button>
        <div>
          <h1 className="text-xl font-semibold">
            {mode === "wizard" ? "Monthly Ingestion Wizard" : "Import Data"}
          </h1>
          {mode === "wizard" && step !== "complete" && (
            <p className="text-sm text-muted-foreground">
              {wizardAccounts.length} account{wizardAccounts.length !== 1 ? "s" : ""} to process
            </p>
          )}
        </div>
      </div>

      <div className="flex gap-6">
        {/* Sidebar progress (wizard mode) */}
        {mode === "wizard" && step !== "complete" && (
          <div className="hidden md:block w-48 shrink-0">
            <WizardProgress
              accounts={wizardAccounts}
              currentIndex={currentIndex}
              completedIds={completedIds}
              skippedIds={skippedIds}
            />
          </div>
        )}

        {/* Main content */}
        <Card className="flex-1">
          <CardContent className="pt-6">
            {/* Account select (single mode) */}
            {step === "account-select" && (
              <div className="space-y-4">
                <div>
                  <h2 className="text-lg font-semibold">Select Account</h2>
                  <p className="text-sm text-muted-foreground">Choose which account to import data into.</p>
                </div>
                <Select value={selectedAccountId ?? ""} onValueChange={(v) => setSelectedAccountId(v)}>
                  <SelectTrigger>
                    <SelectValue placeholder="Select an account" />
                  </SelectTrigger>
                  <SelectContent>
                    {allAccounts.map((a) => (
                      <SelectItem key={a.id} value={a.id}>
                        {a.name} ({a.institution})
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                <div className="flex justify-end">
                  <Button onClick={() => setStep("upload")} disabled={!selectedAccountId}>
                    Continue
                  </Button>
                </div>
              </div>
            )}

            {/* File upload */}
            {step === "upload" && currentAccount && (
              <FileUpload
                files={files}
                onFilesChange={setFiles}
                onSubmit={handleSubmit}
                onSkip={mode === "wizard" ? handleSkip : undefined}
                submitting={submitting}
                accountName={currentAccount.name}
                accountInstitution={currentAccount.institution}
              />
            )}

            {/* Result / Preview */}
            {step === "result" && lastResult && currentAccount && (
              <PreviewTable
                result={lastResult}
                accountName={currentAccount.name}
                onConfirm={handleResultDone}
                onCancel={handleResultDone}
              />
            )}

            {/* Completion summary */}
            {step === "complete" && (
              <ImportSummary
                accountResults={accountResults}
                onImportMore={handleReset}
              />
            )}
          </CardContent>
        </Card>
      </div>
    </div>
  )
}
