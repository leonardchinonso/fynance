import { useEffect, useRef, useState, type DragEvent } from "react"
import { useNavigate } from "react-router-dom"
import { ArrowLeft, Download, Trash2, Upload, AlertTriangle, FileWarning } from "lucide-react"
import { api } from "@/api/client"
import { DocumentReferencedError } from "@/api/service"
import type { DocumentSummary } from "@/bindings/DocumentSummary"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { cn } from "@/lib/utils"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { formatDate } from "@/lib/utils"

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

interface References {
  transactions: number
  holdings: number
  investments: number
}

function referencesSummary(r: References): string {
  const parts: string[] = []
  if (r.transactions > 0) parts.push(`${r.transactions} transaction${r.transactions === 1 ? "" : "s"}`)
  if (r.holdings > 0) parts.push(`${r.holdings} holding${r.holdings === 1 ? "" : "s"}`)
  if (r.investments > 0) parts.push(`${r.investments} investment${r.investments === 1 ? "" : "s"}`)
  return parts.join(", ")
}

export function DocumentsPage() {
  const navigate = useNavigate()
  const [docs, setDocs] = useState<DocumentSummary[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [uploading, setUploading] = useState(false)
  const [dragOver, setDragOver] = useState(false)
  const fileInputRef = useRef<HTMLInputElement>(null)

  // The document pending a force-delete confirmation, plus its reference breakdown.
  const [confirm, setConfirm] = useState<{ doc: DocumentSummary; references: References } | null>(null)
  const [deleting, setDeleting] = useState(false)

  function load() {
    setLoading(true)
    api
      .listDocuments()
      .then((d) => {
        setDocs(d)
        setError(null)
      })
      .catch((e: unknown) => setError(e instanceof Error ? e.message : String(e)))
      .finally(() => setLoading(false))
  }

  useEffect(load, [])

  async function handleUpload(files: FileList | null) {
    if (!files || files.length === 0) return
    setUploading(true)
    setError(null)
    try {
      await api.uploadDocuments(Array.from(files))
      load()
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setUploading(false)
      if (fileInputRef.current) fileInputRef.current.value = ""
    }
  }

  function handleDrop(e: DragEvent) {
    e.preventDefault()
    setDragOver(false)
    handleUpload(e.dataTransfer.files)
  }

  async function handleDelete(doc: DocumentSummary, force: boolean) {
    setDeleting(true)
    setError(null)
    try {
      await api.deleteDocument(doc.id, force)
      setConfirm(null)
      load()
    } catch (e: unknown) {
      if (e instanceof DocumentReferencedError) {
        // Open the confirm dialog with the precise breakdown.
        setConfirm({ doc, references: e.references })
      } else {
        setError(e instanceof Error ? e.message : String(e))
        setConfirm(null)
      }
    } finally {
      setDeleting(false)
    }
  }

  return (
    <div className="max-w-5xl mx-auto py-4">
      <div className="flex items-center gap-3 mb-6">
        <Button variant="ghost" size="icon" className="h-8 w-8" onClick={() => navigate("/reports")}>
          <ArrowLeft className="h-4 w-4" />
        </Button>
        <div className="flex-1 min-w-0">
          <h1 className="text-xl font-semibold">Documents</h1>
          <p className="text-sm text-muted-foreground">
            Every file you've imported, plus any you upload here. Orphaned files were created by a
            parse that was never committed, and are safe to delete.
          </p>
        </div>
        <input
          ref={fileInputRef}
          type="file"
          multiple
          className="hidden"
          onChange={(e) => handleUpload(e.target.files)}
        />
      </div>

      {/* Drag-and-drop upload area (also click-to-browse). */}
      <div
        role="button"
        tabIndex={0}
        aria-label="Upload documents"
        onClick={() => fileInputRef.current?.click()}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault()
            fileInputRef.current?.click()
          }
        }}
        onDragOver={(e) => {
          e.preventDefault()
          setDragOver(true)
        }}
        onDragLeave={() => setDragOver(false)}
        onDrop={handleDrop}
        className={cn(
          "mb-4 flex flex-col items-center justify-center gap-1.5 rounded-xl border-2 border-dashed p-8 text-center transition-colors cursor-pointer outline-none focus-visible:ring-3 focus-visible:ring-ring/50",
          dragOver ? "border-blue-500 bg-blue-500/5" : "border-border hover:border-foreground/30"
        )}
      >
        <Upload className={cn("h-6 w-6", dragOver ? "text-blue-600" : "text-muted-foreground")} />
        <p className="text-sm font-medium">
          {uploading ? "Uploading…" : dragOver ? "Drop to upload" : "Drag files here, or click to browse"}
        </p>
        <p className="text-xs text-muted-foreground">
          Multiple files supported. Up to 10 MB each.
        </p>
      </div>

      {error && (
        <div className="mb-4 flex items-start gap-2 rounded-lg border border-destructive/40 bg-destructive/5 p-3 text-sm">
          <AlertTriangle className="h-4 w-4 text-destructive shrink-0 mt-0.5" />
          <span className="text-xs text-destructive">{error}</span>
        </div>
      )}

      {loading ? (
        <p className="text-sm text-muted-foreground text-center py-12">Loading documents…</p>
      ) : docs.length === 0 ? (
        <div className="rounded-lg border border-dashed p-10 text-center text-sm text-muted-foreground">
          No documents yet. Files you import appear here automatically, or upload one above.
        </div>
      ) : (
        <div className="rounded-xl border overflow-hidden">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>File</TableHead>
                <TableHead>Type</TableHead>
                <TableHead className="text-right">Size</TableHead>
                <TableHead>Origin</TableHead>
                <TableHead>Account</TableHead>
                <TableHead>Uploaded</TableHead>
                <TableHead className="text-right">Links</TableHead>
                <TableHead className="text-right">Actions</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {docs.map((doc) => (
                <TableRow key={doc.id}>
                  <TableCell className="font-medium max-w-[18rem] truncate" title={doc.filename}>
                    <span className="flex items-center gap-2">
                      <span className="truncate">{doc.filename}</span>
                      {doc.orphaned && (
                        <Badge variant="outline" className="gap-1 text-amber-600 border-amber-500/40 shrink-0">
                          <FileWarning className="h-3 w-3" />
                          Orphaned
                        </Badge>
                      )}
                    </span>
                  </TableCell>
                  <TableCell className="text-xs text-muted-foreground">{doc.mime_type}</TableCell>
                  <TableCell className="text-right tabular-nums text-xs">{formatBytes(doc.size_bytes)}</TableCell>
                  <TableCell>
                    <Badge variant="secondary" className="text-xs">{doc.origin}</Badge>
                  </TableCell>
                  <TableCell className="text-xs text-muted-foreground">{doc.account_id ?? "—"}</TableCell>
                  <TableCell className="text-xs text-muted-foreground whitespace-nowrap">
                    {formatDate(doc.uploaded_at)}
                  </TableCell>
                  <TableCell className="text-right tabular-nums text-xs">{doc.reference_count}</TableCell>
                  <TableCell className="text-right">
                    <div className="flex items-center justify-end gap-1">
                      <a
                        href={api.documentDownloadUrl(doc.id)}
                        download={doc.filename}
                        className="inline-flex h-8 w-8 items-center justify-center rounded-md hover:bg-muted"
                        title="Download"
                      >
                        <Download className="h-4 w-4" />
                      </a>
                      <Button
                        variant="ghost"
                        size="icon"
                        className="h-8 w-8 text-destructive hover:text-destructive"
                        title="Delete"
                        onClick={() => handleDelete(doc, false)}
                      >
                        <Trash2 className="h-4 w-4" />
                      </Button>
                    </div>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
      )}

      <Dialog open={!!confirm} onOpenChange={(open) => { if (!open && !deleting) setConfirm(null) }}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Delete a linked document?</DialogTitle>
            <DialogDescription>
              {confirm && (
                <>
                  <span className="font-medium text-foreground">{confirm.doc.filename}</span> is linked to{" "}
                  {referencesSummary(confirm.references)}. Deleting it will remove that link from each of
                  them. The rows themselves are kept. This cannot be undone.
                </>
              )}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setConfirm(null)} disabled={deleting}>
              Cancel
            </Button>
            <Button
              className="bg-destructive text-white hover:bg-destructive/90"
              disabled={deleting}
              onClick={() => confirm && handleDelete(confirm.doc, true)}
            >
              {deleting ? "Deleting…" : "Delete and unlink"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}
