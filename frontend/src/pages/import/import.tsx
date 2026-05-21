import { useState, useEffect } from "react"
import { useSearchParams, useNavigate } from "react-router-dom"
import { api } from "@/api/client"
import type { Account, Profile } from "@/types"
import type { ParseHints } from "@/bindings/ParseHints"
import { useIngestionPreferences } from "@/hooks/use_ingestion_preferences"
import { Card, CardContent } from "@/components/ui/card"
import { Button } from "@/components/ui/button"
import { useProfileColorsContext } from "@/context/profile_colors_context"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { AlertTriangle, CheckCircle2 } from "lucide-react"
import { ReloadingOverlay } from "@/components/reloading_overlay"
import { FileUpload } from "./file_upload"
import { ImportSummary, type AccountResult } from "./import_summary"
import { WizardProgress } from "./wizard_progress"
import { ArrowLeft } from "lucide-react"
import { defaultHintsForAccount } from "./return_type"
import { PreviewReview, type CommitOutcome } from "./preview/preview_review"
import { ClarificationDialog } from "./preview/clarification_dialog"
import { ImportLanding } from "./import_landing"
import { WizardSetup } from "./wizard_setup"
import { RecentImportsList } from "./recent_imports_list"
import { useRecentImports, type RecentImportEntry } from "@/hooks/use_recent_imports"

type Step = "account-select" | "wizard-prep" | "upload" | "preview" | "complete"

/**
 * Pull a readable string out of a fetch error. The api client throws
 * `Error("400 Bad Request: {\"code\":..., \"error\":...}")`; pluck the
 * `error` field out of the JSON body when present.
 */
function extractErrorMessage(err: unknown): string {
  const raw = err instanceof Error ? err.message : String(err)
  const jsonStart = raw.indexOf("{")
  if (jsonStart !== -1) {
    try {
      const body = JSON.parse(raw.slice(jsonStart))
      if (body && typeof body.error === "string") return body.error
    } catch { /* fall through */ }
  }
  return raw
}

function profilesFor(account: Account, profiles: Profile[]): Profile[] {
  if (account.profile_ids.length === 0) return []
  const byId = new Map(profiles.map((p) => [p.id, p]))
  return account.profile_ids
    .map((id) => byId.get(id))
    .filter((p): p is Profile => !!p && p.id !== "default")
}

function formatCompletedTime(tsSec: number): string {
  if (!tsSec) return "in this session"
  const diff = Math.max(0, Date.now() / 1000 - tsSec)
  if (diff < 60) return "just now"
  if (diff < 3600) return `${Math.floor(diff / 60)} min ago`
  if (diff < 86400) return `${Math.floor(diff / 3600)} hr ago`
  return new Date(tsSec * 1000).toLocaleString()
}

function AlreadyImportedBanner({ ts }: { ts: number }) {
  return (
    <div className="flex items-start gap-3 rounded-lg border border-green-600/30 bg-green-600/5 p-3 text-sm">
      <CheckCircle2 className="h-4 w-4 text-green-600 shrink-0 mt-0.5" />
      <div className="flex-1 min-w-0">
        <p className="font-medium">You've already imported this account in this session.</p>
        <p className="text-xs text-muted-foreground mt-0.5">
          Completed {formatCompletedTime(ts)}. Upload again only if you have new data.
        </p>
      </div>
    </div>
  )
}

function AccountLabel({ account, profiles }: { account: Account; profiles: Profile[] }) {
  const accountProfiles = profilesFor(account, profiles)
  const { profileColors } = useProfileColorsContext()
  return (
    <span className="flex items-center gap-2">
      <span className="truncate">
        {account.name} <span className="text-muted-foreground">({account.institution})</span>
      </span>
      {accountProfiles.map((p) => {
        const color = profileColors[p.id] ?? "#78716c"
        return (
          <span
            key={p.id}
            className="inline-flex items-center text-[10px] py-0 px-1.5 h-4 font-normal shrink-0 rounded-full border tabular-nums"
            style={{
              backgroundColor: `${color}24`,
              borderColor: `${color}66`,
              color,
            }}
          >
            {p.name}
          </span>
        )
      })}
    </span>
  )
}

export function ImportPage() {
  const [searchParams] = useSearchParams()
  const navigate = useNavigate()
  const modeParam = searchParams.get("mode")
  // No mode = landing page. Treat anything other than "wizard" as single.
  const mode = modeParam === "wizard" ? "wizard" : "single"
  const showLanding = modeParam !== "single" && modeParam !== "wizard"

  const [allAccounts, setAllAccounts] = useState<Account[]>([])
  const [profiles, setProfiles] = useState<Profile[]>([])
  const [loading, setLoading] = useState(true)
  const {
    getOrderedAccounts,
    getHiddenAccounts,
    showAccount,
    hideAccount,
    reorderAccounts,
  } = useIngestionPreferences()

  // Session-only state. Anything navigational lives in the URL (below).
  // `accountResults` keeps per-account outcome details (counts, errors) for
  // the final summary — they can't be encoded in the URL, so a refresh loses
  // them, but `completedIds` is URL-driven so wizard progress survives.
  const [selectedAccountId, setSelectedAccountId] = useState<string | null>(null)
  const [files, setFiles] = useState<File[]>([])
  const [parsing, setParsing] = useState(false)
  const [parseError, setParseError] = useState<string | null>(null)
  const [accountResults, setAccountResults] = useState<AccountResult[]>([])
  /** Parse hints the user has chosen for the active account. Reset on account flip. */
  const [hints, setHints] = useState<ParseHints | null>(null)
  const recents = useRecentImports()

  const { syncProfiles } = useProfileColorsContext()

  useEffect(() => {
    Promise.all([api.getAccounts(), api.getProfiles()])
      .then(([accts, profs]) => {
        setAllAccounts(accts)
        setProfiles(profs)
        syncProfiles(profs.map((p) => p.id))
        setLoading(false)
      })
      .catch(() => setLoading(false))
  }, [syncProfiles])

  // Reset purely-session state when the mode flips. Step/preview/index are
  // URL-derived.
  useEffect(() => {
    setFiles([])
    setParseError(null)
    setAccountResults([])
    setSelectedAccountId(null)
  }, [mode])

  /**
   * URL is the source of truth for step / account / preview. Everything
   * navigational is *derived* from the URL on each render rather than
   * mirrored into state via useEffect (the mirror pattern caused render
   * loops and made browser-back stop working).
   *
   * Single mode:
   *   ?mode=single                              → account-select
   *   ?mode=single&account=<id>                 → upload
   *   ?mode=single&account=<id>&entry=<entryId> → preview (from cache)
   *
   * Wizard mode:
   *   ?mode=wizard                              → wizard-prep
   *   ?mode=wizard&account=<id>                 → upload (id must be in queue)
   *   ?mode=wizard&account=<id>&entry=<entryId> → preview (from cache)
   *   ?mode=wizard&done=1                       → completion summary
   *
   * Wizard progress is also encoded via `&completed=<id>,<id>,...` so a
   * refresh keeps the success markers on already-finished accounts. Skipped
   * accounts are derived (before currentIndex, not in completed).
   */
  const accountParam = searchParams.get("account")
  const entryParam = searchParams.get("entry")
  const doneParam = searchParams.get("done") === "1"
  // `completed=<id>:<unixSec>,<id>:<unixSec>` — keeps both membership and a
  // timestamp so a refresh can still render "imported 5 min ago" banners.
  // Tolerates bare ids without timestamps (treated as unknown time).
  const completedParam = searchParams.get("completed")
  const completedAt: Map<string, number> = (() => {
    const m = new Map<string, number>()
    if (!completedParam) return m
    for (const part of completedParam.split(",").filter(Boolean)) {
      const [id, ts] = part.split(":")
      if (id) m.set(id, ts ? Number(ts) : 0)
    }
    return m
  })()
  const completedIds: Set<string> = new Set(completedAt.keys())

  const wizardAccounts = getOrderedAccounts(allAccounts)

  // Derive currentAccount / currentEntry in one pass. Invalid params (account
  // not in list, entry stale) are silently ignored — the step derivation below
  // falls back to whatever the URL prefix still describes.
  let currentAccount: Account | null = null
  let currentEntry: RecentImportEntry | null = null
  let currentIndex = 0

  // Look the entry up once in the already-in-memory `recents.entries` array
  // (parsed once when the hook reads localStorage, kept in sync via the
  // storage-event listener). Using `getById` here would re-parse the entire
  // localStorage payload on every render, which is a real cost when previews
  // are large.
  const entryFromUrl = entryParam
    ? recents.entries.find((e) => e.id === entryParam) ?? null
    : null

  if (!loading) {
    if (mode === "wizard") {
      if (accountParam) {
        const idx = wizardAccounts.findIndex((a) => a.id === accountParam)
        if (idx !== -1) {
          currentIndex = idx
          currentAccount = wizardAccounts[idx]
          if (entryFromUrl && entryFromUrl.accountId === accountParam) {
            currentEntry = entryFromUrl
          }
        }
      }
    } else if (accountParam) {
      currentAccount = allAccounts.find((a) => a.id === accountParam) ?? null
      if (currentAccount && entryFromUrl && entryFromUrl.accountId === accountParam) {
        currentEntry = entryFromUrl
      }
    }
  }

  // Derive the step the same way.
  let step: Step
  if (mode === "wizard") {
    if (doneParam && accountResults.length > 0) step = "complete"
    else if (!accountParam) step = "wizard-prep"
    else if (currentEntry) step = "preview"
    else step = "upload"
  } else {
    if (doneParam && accountResults.length > 0) step = "complete"
    else if (!currentAccount) step = "account-select"
    else if (currentEntry) step = "preview"
    else step = "upload"
  }

  const preview = currentEntry?.preview ?? null
  const currentEntryId = currentEntry?.id ?? null

  // Derive "skipped" accounts directly: any wizard account before the current
  // one that hasn't been completed in this session is treated as skipped. This
  // way a refresh that lands on account #3 with empty session state still
  // shows #1 and #2 with the skipped icon — no extra state syncing needed.
  const skippedIds: Set<string> = (() => {
    if (mode !== "wizard" || !accountParam || currentIndex === 0) return new Set()
    const ids = new Set<string>()
    for (let i = 0; i < currentIndex; i++) {
      const id = wizardAccounts[i].id
      if (!completedIds.has(id)) ids.add(id)
    }
    return ids
  })()

  // Re-default hints whenever we switch to a different account. Investment
  // accounts get investments=true; everything else gets transactions=true.
  // Declared here (not earlier) because currentAccount is derived above.
  const currentAccountId = currentAccount?.id ?? null
  const currentAccountType = currentAccount?.type
  useEffect(() => {
    if (!currentAccountId || !currentAccountType) {
      setHints(null)
      return
    }
    setHints(defaultHintsForAccount(currentAccountType))
  }, [currentAccountId, currentAccountType])

  function navStep(params: {
    account?: string | null
    entry?: string | null
    done?: boolean
    /** Overwrite the completed map; omit to carry forward the current one. */
    completedAt?: Map<string, number>
  }) {
    const sp = new URLSearchParams()
    sp.set("mode", mode)
    if (params.account) sp.set("account", params.account)
    if (params.entry) sp.set("entry", params.entry)
    if (params.done) sp.set("done", "1")
    const map = params.completedAt ?? completedAt
    if (map.size > 0) {
      sp.set(
        "completed",
        [...map.entries()].map(([id, ts]) => (ts ? `${id}:${ts}` : id)).join(",")
      )
    }
    navigate(`/import?${sp.toString()}`)
  }

  async function callParse(filesToSend: File[], hints: ParseHints) {
    if (!currentAccount) return
    setParsing(true)
    setParseError(null)
    try {
      const result = await api.parseDocuments(filesToSend, currentAccount.id, hints)
      const id = recents.add({
        accountId: currentAccount.id,
        fileNames: filesToSend.map((f) => f.name),
        preview: result,
      })
      navStep({ account: currentAccount.id, entry: id })
    } catch (err) {
      console.error("Parse failed:", err)
      setParseError(extractErrorMessage(err))
    } finally {
      setParsing(false)
    }
  }

  function handleResume(entry: RecentImportEntry) {
    navStep({ account: entry.accountId, entry: entry.id })
  }

  async function handleUploadSubmit() {
    if (!currentAccount || files.length === 0) return
    await callParse(files, hints ?? defaultHintsForAccount(currentAccount.type))
  }

  async function handleClarificationRetry(answers: Record<string, string>) {
    if (!currentAccount) return
    const baseHints = defaultHintsForAccount(currentAccount.type)
    const merged: ParseHints = {
      ...baseHints,
      hint: Object.entries(answers).map(([file, ans]) => `${file}: ${ans}`).join("; "),
    }
    await callParse(files, merged)
  }

  function handleCommitted(outcome: CommitOutcome) {
    if (!currentAccount) return
    if (currentEntryId) {
      recents.remove(currentEntryId)
    }
    setAccountResults((prev) => [...prev, {
      accountId: currentAccount.id,
      accountName: currentAccount.name,
      outcome,
      skipped: false,
    }])
    advanceToNext({ markCurrentCompleted: true })
  }

  function handlePreviewCancel() {
    // Drop the entry param; URL effect will switch the user back to the upload step.
    if (currentAccount) navStep({ account: currentAccount.id })
    else navStep({})
  }

  function handleSkip() {
    if (!currentAccount) return
    setAccountResults((prev) => [...prev, {
      accountId: currentAccount.id,
      accountName: currentAccount.name,
      outcome: null,
      skipped: true,
    }])
    advanceToNext()
  }

  function advanceToNext(opts: { markCurrentCompleted?: boolean } = {}) {
    setFiles([])
    setParseError(null)
    let nextMap = completedAt
    if (opts.markCurrentCompleted && currentAccount) {
      nextMap = new Map(completedAt)
      nextMap.set(currentAccount.id, Math.floor(Date.now() / 1000))
    }
    if (mode === "wizard") {
      const nextIndex = currentIndex + 1
      if (nextIndex >= wizardAccounts.length) {
        navStep({ done: true, completedAt: nextMap })
      } else {
        navStep({ account: wizardAccounts[nextIndex].id, completedAt: nextMap })
      }
    } else {
      navStep({ done: true, completedAt: nextMap })
    }
  }

  function handleReset() {
    setFiles([])
    setParseError(null)
    setAccountResults([])
    navStep({ completedAt: new Map() })
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

  if (showLanding) {
    return <ImportLanding />
  }

  const isPreviewing = step === "preview"
  const containerWidth = isPreviewing ? "max-w-[88rem]" : "max-w-3xl"

  function handleBack() {
    setFiles([])
    setParseError(null)
    // From preview, back goes one step: drop the entry param so the user
    // lands on the same account's upload screen, regardless of mode.
    if (step === "preview" && currentAccount) {
      navStep({ account: currentAccount.id })
      return
    }
    // From upload in wizard mode, back goes to the wizard-prep screen.
    if (mode === "wizard" && (step === "upload" || step === "complete")) {
      navStep({})
      return
    }
    // From upload or account-select in single mode, leave the import flow.
    navigate("/import")
  }

  // Recent imports visible on the current step. Wizard always pins to the
  // active account; single mode shows all until an account is picked, then
  // narrows to it.
  const visibleRecents = (() => {
    if (mode === "wizard") {
      return currentAccount
        ? recents.entries.filter((e) => e.accountId === currentAccount.id)
        : []
    }
    if (selectedAccountId) {
      return recents.entries.filter((e) => e.accountId === selectedAccountId)
    }
    return recents.entries
  })()

  return (
    <div className={`${containerWidth} mx-auto py-4`}>
      <div className="flex items-center gap-3 mb-6">
        <Button variant="ghost" size="icon" className="h-8 w-8" onClick={handleBack}>
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
        {mode === "wizard" && step !== "complete" && step !== "wizard-prep" && (
          <div className="hidden md:block w-48 shrink-0">
            <WizardProgress
              accounts={wizardAccounts}
              currentIndex={currentIndex}
              completedIds={completedIds}
              skippedIds={skippedIds}
              onSelectAccount={(a) => navStep({ account: a.id })}
            />
          </div>
        )}

        <div className="flex-1 min-w-0">
        <Card>
          <CardContent className="pt-6">
            {step === "wizard-prep" && mode === "wizard" && (
              <WizardSetup
                accounts={allAccounts}
                profiles={profiles}
                queued={getOrderedAccounts(allAccounts)}
                hidden={getHiddenAccounts(allAccounts)}
                onShowAccount={(id) => showAccount(id, allAccounts)}
                onHideAccount={(id) => hideAccount(id, allAccounts)}
                onReorder={reorderAccounts}
                onContinue={() => {
                  const first = getOrderedAccounts(allAccounts)[0]
                  if (first) navStep({ account: first.id })
                }}
                onCancel={() => navigate("/import")}
              />
            )}

            {step === "account-select" && mode === "single" && (
              <div className="space-y-4">
                <div>
                  <h2 className="text-lg font-semibold">Select Account</h2>
                  <p className="text-sm text-muted-foreground">Choose which account to import data into.</p>
                </div>
                <Select
                  value={selectedAccountId ?? ""}
                  onValueChange={(v) => setSelectedAccountId(v)}
                  items={Object.fromEntries(
                    allAccounts.map((a) => [
                      a.id,
                      <AccountLabel key={a.id} account={a} profiles={profiles} />,
                    ])
                  )}
                >
                  <SelectTrigger className="w-full">
                    <SelectValue placeholder="Select an account" />
                  </SelectTrigger>
                  <SelectContent>
                    {allAccounts.map((a) => (
                      <SelectItem key={a.id} value={a.id}>
                        <AccountLabel account={a} profiles={profiles} />
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                <div className="flex justify-end">
                  <Button
                    onClick={() => { if (selectedAccountId) navStep({ account: selectedAccountId }) }}
                    disabled={!selectedAccountId}
                    className="bg-blue-600 text-white hover:bg-blue-600/90"
                  >
                    Continue
                  </Button>
                </div>
              </div>
            )}

            {step === "upload" && currentAccount && (
              <div className="space-y-3">
                {completedAt.has(currentAccount.id) && (
                  <AlreadyImportedBanner ts={completedAt.get(currentAccount.id)!} />
                )}
                <FileUpload
                  files={files}
                  onFilesChange={setFiles}
                  hints={hints ?? defaultHintsForAccount(currentAccount.type)}
                  onHintsChange={setHints}
                  onSubmit={handleUploadSubmit}
                  onSkip={mode === "wizard" ? handleSkip : undefined}
                  submitting={parsing}
                  accountName={currentAccount.name}
                  accountInstitution={currentAccount.institution}
                />
                {parseError && (
                  <div className="flex items-start gap-2 rounded-lg border border-destructive/40 bg-destructive/5 p-3 text-sm">
                    <AlertTriangle className="h-4 w-4 text-destructive shrink-0 mt-0.5" />
                    <span className="text-xs text-destructive">{parseError}</span>
                  </div>
                )}
              </div>
            )}

            {step === "preview" && preview && currentAccount && currentEntry && preview.status !== "needs_clarification" && (
              <PreviewReview
                preview={preview}
                accountName={currentAccount.name}
                fileCount={files.length || currentEntry.fileNames.length}
                initialEdits={currentEntry.edits}
                onEditsChanged={(edits) => {
                  recents.updateEdits(currentEntry.id, edits)
                }}
                onCommitted={handleCommitted}
                onCancel={handlePreviewCancel}
              />
            )}

            {step === "complete" && (
              <ImportSummary
                accountResults={accountResults}
                onImportMore={handleReset}
              />
            )}
          </CardContent>
        </Card>

        {(step === "account-select" || step === "upload") && (
          <RecentImportsList
            entries={visibleRecents}
            accounts={allAccounts}
            profiles={profiles}
            onResume={handleResume}
            onDiscard={(id) => recents.remove(id)}
          />
        )}
        </div>
      </div>

      {preview && preview.status === "needs_clarification" && step === "preview" && (
        <ClarificationDialog
          requests={preview.clarifications_needed}
          retrying={parsing}
          onRetry={handleClarificationRetry}
          onCancel={() => { if (currentAccount) navStep({ account: currentAccount.id }); else navStep({}) }}
        />
      )}

      <ReloadingOverlay
        active={parsing}
        fullscreen
        text="Currently importing data. Please wait, this might take a while."
      />
    </div>
  )
}
