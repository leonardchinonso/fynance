import { useEffect, useRef, useState } from "react"
import { api } from "@/api/client"
import type { IngestionPreview } from "@/bindings/IngestionPreview"
import type { ImportPayload } from "@/bindings/ImportPayload"
import type { HoldingsImportPayload } from "@/bindings/HoldingsImportPayload"
import type { InvestmentsImportPayload } from "@/bindings/InvestmentsImportPayload"
import type { Currency } from "@/types"
import type { PreviewEdits } from "@/hooks/use_recent_imports"
import { Button } from "@/components/ui/button"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { CountBadge } from "@/components/count_badge"
import { AlertTriangle } from "lucide-react"
import { useUrlState } from "@/hooks/use_url_state"
import { MetadataHeader } from "./metadata_header"
import { TransactionsSection } from "./transactions_section"
import { HoldingsSection } from "./holdings_section"
import { InvestmentsSection } from "./investments_section"
import { ConfirmDialog } from "./confirm_dialog"

export interface CommitOutcome {
  transactionsInserted: number
  transactionsDuplicate: number
  holdingsInserted: number
  holdingsUpdated: number
  investmentsInserted: number
  investmentsDuplicate: number
  errors: string[]
}

interface Props {
  preview: IngestionPreview
  accountName: string
  fileCount: number
  /** Seed edits when resuming a previously-cached import. */
  initialEdits?: PreviewEdits
  /** Fired on every edit so the parent can persist to the LRU cache. */
  onEditsChanged?: (edits: PreviewEdits) => void
  /** Called when the import committed successfully (errors.length === 0). */
  onCommitted: (outcome: CommitOutcome) => void
  /** Called when the user cancels and wants to go back to upload. */
  onCancel: () => void
}

type Tab = "transactions" | "holdings" | "investments"

export function PreviewReview({
  preview,
  accountName,
  fileCount,
  initialEdits,
  onEditsChanged,
  onCommitted,
  onCancel,
}: Props) {
  const [txPayload, setTxPayload] = useState<ImportPayload | null>(
    initialEdits?.txPayload ?? preview.transactions.payload
  )
  const [holdingsPayload, setHoldingsPayload] = useState<HoldingsImportPayload | null>(
    initialEdits?.holdingsPayload ?? preview.holdings.payload
  )
  const [invPayload, setInvPayload] = useState<InvestmentsImportPayload | null>(
    initialEdits?.invPayload ?? preview.investments.payload
  )
  const [txDeleted, setTxDeleted] = useState<Set<number>>(new Set(initialEdits?.txDeleted ?? []))
  const [holdingsDeleted, setHoldingsDeleted] = useState<Set<number>>(
    new Set(initialEdits?.holdingsDeleted ?? [])
  )
  const [invDeleted, setInvDeleted] = useState<Set<number>>(new Set(initialEdits?.invDeleted ?? []))

  // Skip the first emit so we don't immediately re-write the cache with the
  // same values it was just seeded from.
  const skipFirstEmit = useRef(true)
  // Stash the latest onEditsChanged in a ref so it isn't part of the effect
  // deps — including it there would re-fire the effect every parent render
  // (the parent rebuilds the arrow each render), which loops the cache write.
  const onEditsChangedRef = useRef(onEditsChanged)
  onEditsChangedRef.current = onEditsChanged
  useEffect(() => {
    if (skipFirstEmit.current) {
      skipFirstEmit.current = false
      return
    }
    onEditsChangedRef.current?.({
      txPayload,
      holdingsPayload,
      invPayload,
      txDeleted: Array.from(txDeleted),
      holdingsDeleted: Array.from(holdingsDeleted),
      invDeleted: Array.from(invDeleted),
    })
  }, [txPayload, holdingsPayload, invPayload, txDeleted, holdingsDeleted, invDeleted])

  const [categoryById, setCategoryById] = useState<Record<string, string>>({})
  const [currencyOptions, setCurrencyOptions] = useState<Currency[]>([])

  const [confirmOpen, setConfirmOpen] = useState(false)
  const [submitting, setSubmitting] = useState(false)
  const [submitError, setSubmitError] = useState<string | null>(null)

  useEffect(() => {
    api.getCategoriesWithIds()
      .then((entries) => {
        const map: Record<string, string> = {}
        for (const e of entries) map[e.id] = e.name
        setCategoryById(map)
      })
      .catch(() => setCategoryById({}))
    api.getCurrencies().then(setCurrencyOptions).catch(() => setCurrencyOptions([]))
  }, [])

  const showTx = preview.transactions.count > 0
  const showHoldings = preview.holdings.count > 0
  const showInv = preview.investments.count > 0
  const sectionTabs: Tab[] = []
  if (showTx) sectionTabs.push("transactions")
  if (showHoldings) sectionTabs.push("holdings")
  if (showInv) sectionTabs.push("investments")

  const url = useUrlState()
  const fallbackTab = sectionTabs[0] ?? "transactions"
  const urlTab = url.get("tab", "")
  const tab: Tab = sectionTabs.includes(urlTab as Tab) ? (urlTab as Tab) : fallbackTab
  const setTab = (next: Tab) => url.set({ tab: next === fallbackTab ? null : next })

  function buildCommitPayloads() {
    const txOut = txPayload && {
      ...txPayload,
      transactions: txPayload.transactions.filter((_, i) => !txDeleted.has(i)),
    }
    const holdingsOut = holdingsPayload && {
      ...holdingsPayload,
      holdings: holdingsPayload.holdings.filter((_, i) => !holdingsDeleted.has(i)),
    }
    const invOut = invPayload && {
      ...invPayload,
      events: invPayload.events.filter((_, i) => !invDeleted.has(i)),
    }
    return { txOut, holdingsOut, invOut }
  }

  function commitCounts() {
    const txCount = txPayload ? txPayload.transactions.length - txDeleted.size : 0
    const holdingsTotal = holdingsPayload
      ? holdingsPayload.holdings.length - holdingsDeleted.size
      : 0
    // Split create vs update by checking each survivor against the preview status.
    let holdingsCreate = 0
    let holdingsUpdate = 0
    if (holdingsPayload) {
      let n = 0
      preview.holdings.rows.forEach((r) => {
        const isActionable = r.status === "new" || r.status === "modify"
        if (!isActionable) return
        const payloadIdx = n
        n += 1
        if (holdingsDeleted.has(payloadIdx)) return
        if (r.status === "modify") holdingsUpdate += 1
        else holdingsCreate += 1
      })
    }
    const invCount = invPayload ? invPayload.events.length - invDeleted.size : 0
    return {
      transactions: Math.max(0, txCount),
      holdingsCreate: Math.max(0, holdingsCreate),
      holdingsUpdate: Math.max(0, holdingsUpdate),
      investments: Math.max(0, invCount),
      total: Math.max(0, txCount + holdingsTotal + invCount),
    }
  }

  async function handleConfirm() {
    setSubmitting(true)
    setSubmitError(null)
    const { txOut, holdingsOut, invOut } = buildCommitPayloads()
    const outcome: CommitOutcome = {
      transactionsInserted: 0,
      transactionsDuplicate: 0,
      holdingsInserted: 0,
      holdingsUpdated: 0,
      investmentsInserted: 0,
      investmentsDuplicate: 0,
      errors: [],
    }
    const calls: Promise<void>[] = []
    if (txOut && txOut.transactions.length > 0) {
      calls.push(
        api.commitTransactions(txOut).then((r) => {
          outcome.transactionsInserted = Number(r.rows_inserted)
          outcome.transactionsDuplicate = Number(r.rows_duplicate)
          // Backend uses `#[serde(skip_serializing_if = "Vec::is_empty")]` on
          // `errors`, so the field is absent from the JSON when there are
          // none. Treat missing/null as an empty list.
          const errs = r.errors ?? []
          if (errs.length > 0) {
            outcome.errors.push(
              ...errs.map((e) => `Transaction row ${e.index + 1}: ${e.reason}`)
            )
          }
        }).catch((e: unknown) => {
          outcome.errors.push(`Transactions import failed: ${String(e)}`)
        })
      )
    }
    if (holdingsOut && holdingsOut.holdings.length > 0) {
      calls.push(
        api.commitHoldings(holdingsOut).then((r) => {
          outcome.holdingsInserted = r.inserted
          outcome.holdingsUpdated = r.updated
        }).catch((e: unknown) => {
          outcome.errors.push(`Holdings import failed: ${String(e)}`)
        })
      )
    }
    if (invOut && invOut.events.length > 0) {
      calls.push(
        api.commitInvestments(invOut).then((r) => {
          outcome.investmentsInserted = r.inserted
          outcome.investmentsDuplicate = r.duplicates
          const errs = r.errors ?? []
          if (errs.length > 0) {
            outcome.errors.push(
              ...errs.map((e) => `Investment row ${e.index + 1}: ${e.reason}`)
            )
          }
        }).catch((e: unknown) => {
          outcome.errors.push(`Investments import failed: ${String(e)}`)
        })
      )
    }

    await Promise.all(calls)
    setSubmitting(false)
    // Always close the confirm dialog after a submit attempt. Errors are
    // surfaced via the inline banner on the main page so the user can read,
    // copy, or screenshot them; keeping the dialog open over the error was
    // its own UX bug.
    setConfirmOpen(false)
    if (outcome.errors.length === 0) {
      onCommitted(outcome)
    } else {
      setSubmitError(outcome.errors.join("\n"))
    }
  }

  const counts = commitCounts()

  return (
    <div className="space-y-4">
      <div>
        <h2 className="text-lg font-semibold">Review {accountName}</h2>
        <p className="text-sm text-muted-foreground">
          Edit rows in place. Click <strong>Submit</strong> when ready.
        </p>
      </div>

      <MetadataHeader metadata={preview.metadata} fileCount={fileCount} />

      {sectionTabs.length === 0 ? (
        <div className="rounded-lg border border-dashed p-6 text-center text-sm text-muted-foreground">
          No data was extracted from the uploaded files.
        </div>
      ) : sectionTabs.length === 1 ? (
        <div>
          {tab === "transactions" && showTx && (
            <TransactionsSection
              result={preview.transactions}
              payload={txPayload}
              setPayload={setTxPayload}
              markedForDeletion={txDeleted}
              setMarkedForDeletion={setTxDeleted}
              categoryById={categoryById}
              currencyOptions={currencyOptions}
            />
          )}
          {tab === "holdings" && showHoldings && (
            <HoldingsSection
              result={preview.holdings}
              payload={holdingsPayload}
              setPayload={setHoldingsPayload}
              markedForDeletion={holdingsDeleted}
              setMarkedForDeletion={setHoldingsDeleted}
              currencyOptions={currencyOptions}
            />
          )}
          {tab === "investments" && showInv && (
            <InvestmentsSection
              result={preview.investments}
              payload={invPayload}
              setPayload={setInvPayload}
              markedForDeletion={invDeleted}
              setMarkedForDeletion={setInvDeleted}
              currencyOptions={currencyOptions}
            />
          )}
        </div>
      ) : (
        <Tabs value={tab} onValueChange={(v) => setTab(v as Tab)}>
          <TabsList className="grid w-full" style={{ gridTemplateColumns: `repeat(${sectionTabs.length}, 1fr)` }}>
            {showTx && (
              <TabsTrigger value="transactions" className="gap-1.5">
                Transactions <CountBadge count={preview.transactions.count} />
              </TabsTrigger>
            )}
            {showHoldings && (
              <TabsTrigger value="holdings" className="gap-1.5">
                Holdings <CountBadge count={preview.holdings.count} />
              </TabsTrigger>
            )}
            {showInv && (
              <TabsTrigger value="investments" className="gap-1.5">
                Investments <CountBadge count={preview.investments.count} />
              </TabsTrigger>
            )}
          </TabsList>
          {/* Only mount the active tab's heavy section. Tables render dozens
              to hundreds of rows; mounting all three on first paint is the
              single biggest contributor to the "click recent → page lag"
              the user noticed. */}
          {showTx && (
            <TabsContent value="transactions" className="mt-4">
              {tab === "transactions" && (
                <TransactionsSection
                  result={preview.transactions}
                  payload={txPayload}
                  setPayload={setTxPayload}
                  markedForDeletion={txDeleted}
                  setMarkedForDeletion={setTxDeleted}
                  categoryById={categoryById}
                  currencyOptions={currencyOptions}
                />
              )}
            </TabsContent>
          )}
          {showHoldings && (
            <TabsContent value="holdings" className="mt-4">
              {tab === "holdings" && (
                <HoldingsSection
                  result={preview.holdings}
                  payload={holdingsPayload}
                  setPayload={setHoldingsPayload}
                  markedForDeletion={holdingsDeleted}
                  setMarkedForDeletion={setHoldingsDeleted}
                  currencyOptions={currencyOptions}
                />
              )}
            </TabsContent>
          )}
          {showInv && (
            <TabsContent value="investments" className="mt-4">
              {tab === "investments" && (
                <InvestmentsSection
                  result={preview.investments}
                  payload={invPayload}
                  setPayload={setInvPayload}
                  markedForDeletion={invDeleted}
                  setMarkedForDeletion={setInvDeleted}
                  currencyOptions={currencyOptions}
                />
              )}
            </TabsContent>
          )}
        </Tabs>
      )}

      {submitError && (
        <div className="flex items-start gap-2 rounded-lg border border-destructive/40 bg-destructive/5 p-3 text-sm">
          <AlertTriangle className="h-4 w-4 text-destructive shrink-0 mt-0.5" />
          <pre className="whitespace-pre-wrap font-sans text-xs text-destructive">{submitError}</pre>
        </div>
      )}

      <div className="flex items-center justify-end gap-2 border-t pt-4">
        <Button variant="outline" onClick={onCancel} disabled={submitting}>
          Back
        </Button>
        <Button
          onClick={() => setConfirmOpen(true)}
          disabled={submitting || counts.total === 0}
          className="gap-1.5 bg-blue-600 text-white hover:bg-blue-600/90"
        >
          Submit <CountBadge count={counts.total} className="bg-white/20 text-white" />
        </Button>
      </div>

      <ConfirmDialog
        open={confirmOpen}
        onOpenChange={(open) => { if (!submitting) setConfirmOpen(open) }}
        counts={counts}
        submitting={submitting}
        onConfirm={handleConfirm}
      />
    </div>
  )
}
