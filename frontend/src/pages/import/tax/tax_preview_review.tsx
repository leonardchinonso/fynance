import { useEffect, useMemo, useRef, useState } from "react"
import { AlertTriangle } from "lucide-react"
import { api } from "@/api/client"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import type { SourceDocMeta } from "@/components/source_chips"
import type { IngestionPreview } from "@/bindings/IngestionPreview"
import type { TaxDocumentDraft } from "@/bindings/TaxDocumentDraft"
import type { TaxDocumentIngestionResult } from "@/bindings/TaxDocumentIngestionResult"
import type { TaxDocumentsImportResult } from "@/bindings/TaxDocumentsImportResult"
import type { TaxEdits } from "@/hooks/use_recent_imports"
import { MetadataHeader } from "../preview/metadata_header"
import { ReconciliationBanner } from "./reconciliation_banner"
import { TaxDocumentsTable } from "./tax_documents_table"
import { buildCommitPayload, defaultKept } from "./tax_drafts"

interface Props {
  preview: IngestionPreview
  /** The `tax_documents` section of `preview`; the caller has checked it exists. */
  result: TaxDocumentIngestionResult
  fileCount: number
  /** Seed edits when resuming a cached import. */
  initialEdits?: TaxEdits
  /** Fired on every edit so the parent can persist to the recent-imports cache. */
  onEditsChanged?: (edits: TaxEdits) => void
  /** Called once the backend saved the records without errors. */
  onCommitted: (result: TaxDocumentsImportResult) => void
  onCancel: () => void
}

export function TaxPreviewReview({
  preview,
  result,
  fileCount,
  initialEdits,
  onEditsChanged,
  onCommitted,
  onCancel,
}: Props) {
  const [drafts, setDrafts] = useState<TaxDocumentDraft[]>(
    () =>
      initialEdits && initialEdits.drafts.length === result.rows.length
        ? initialEdits.drafts
        : result.rows.map((r) => r.draft)
  )
  const [kept, setKept] = useState<Set<number>>(
    () =>
      new Set(
        initialEdits ? initialEdits.kept : result.rows.filter(defaultKept).map((r) => r.index)
      )
  )
  const [confirmOpen, setConfirmOpen] = useState(false)
  const [submitting, setSubmitting] = useState(false)
  const [submitError, setSubmitError] = useState<string | null>(null)

  // Skip the first emit so the cache is not rewritten with the values it seeded.
  const skipFirstEmit = useRef(true)
  const onEditsChangedRef = useRef(onEditsChanged)
  onEditsChangedRef.current = onEditsChanged
  useEffect(() => {
    if (skipFirstEmit.current) {
      skipFirstEmit.current = false
      return
    }
    onEditsChangedRef.current?.({ drafts, kept: Array.from(kept) })
  }, [drafts, kept])

  const rows = useMemo(
    () => result.rows.map((r, i) => ({ ...r, draft: drafts[i] ?? r.draft })),
    [result.rows, drafts]
  )
  const docsMap = useMemo(() => {
    const m = new Map<string, SourceDocMeta>()
    for (const d of preview.documents ?? []) {
      m.set(d.id, { filename: d.filename, uploaded_at: d.uploaded_at })
    }
    return m
  }, [preview.documents])

  const payload = buildCommitPayload(result, rows, kept)
  const count = payload.documents.length

  function toggleKept(rowIndex: number) {
    setKept((prev) => {
      const next = new Set(prev)
      if (next.has(rowIndex)) next.delete(rowIndex)
      else next.add(rowIndex)
      return next
    })
  }

  async function handleConfirm() {
    setSubmitting(true)
    setSubmitError(null)
    // The backend reports errors by position in the documents it received.
    const sentRows = rows.filter((r) => kept.has(r.index) && r.status !== "error")
    try {
      const res = await api.commitTaxDocuments(payload)
      // All-or-nothing: any per-row error means nothing was saved.
      const errors = res.errors ?? []
      if (errors.length > 0) {
        setConfirmOpen(false)
        setSubmitError(
          "Nothing was saved.\n" +
            errors
              .map((e) => `Row ${(sentRows[e.index]?.index ?? e.index) + 1}: ${e.reason}`)
              .join("\n")
        )
      } else {
        onCommitted(res)
      }
    } catch (e: unknown) {
      setConfirmOpen(false)
      setSubmitError(`Saving failed: ${e instanceof Error ? e.message : String(e)}`)
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <div className="space-y-4">
      <div>
        <h2 className="text-lg font-semibold">Review tax documents</h2>
        <p className="text-sm text-muted-foreground">
          Edit amounts in place, untick rows you do not want, then save. Year-to-date figures are
          kept for reference and never added up.
        </p>
      </div>

      <MetadataHeader metadata={preview.metadata} fileCount={fileCount} />

      <ReconciliationBanner entries={result.reconciliation} />

      {rows.length === 0 ? (
        <div className="rounded-lg border border-dashed p-6 text-center text-sm text-muted-foreground">
          No payslips or P60s were found in the uploaded files.
        </div>
      ) : (
        <TaxDocumentsTable
          rows={rows}
          kept={kept}
          onToggleKept={toggleKept}
          onDraftChange={(pos, draft) =>
            setDrafts((prev) => prev.map((d, i) => (i === pos ? draft : d)))
          }
          knownSources={result.known_sources}
          docs={docsMap}
          disabled={submitting}
        />
      )}

      {submitError && (
        <div className="flex items-start gap-2 rounded-lg border border-destructive/40 bg-destructive/5 p-3 text-sm">
          <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0 text-destructive" />
          <span className="whitespace-pre-wrap text-xs text-destructive">{submitError}</span>
        </div>
      )}

      <div className="flex items-center justify-between">
        <Button variant="outline" onClick={onCancel} disabled={submitting}>
          Back
        </Button>
        <Button
          onClick={() => setConfirmOpen(true)}
          disabled={count === 0 || submitting}
          className="bg-blue-600 text-white hover:bg-blue-600/90"
        >
          Save {count} record{count !== 1 ? "s" : ""}
        </Button>
      </div>

      <Dialog open={confirmOpen} onOpenChange={(o) => { if (!submitting) setConfirmOpen(o) }}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Save tax records</DialogTitle>
            <DialogDescription>
              {count} record{count !== 1 ? "s" : ""} will be saved to your tax records. If any row
              is rejected, nothing is saved.
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setConfirmOpen(false)} disabled={submitting}>
              Cancel
            </Button>
            <Button onClick={handleConfirm} disabled={submitting}>
              {submitting ? "Saving…" : "Confirm"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}
