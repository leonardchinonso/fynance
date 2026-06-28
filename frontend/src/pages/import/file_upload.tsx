import { useState, useRef, useEffect, type DragEvent } from "react"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { Textarea } from "@/components/ui/textarea"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Upload, X, FileText, ChevronRight, AlertTriangle } from "lucide-react"
import { cn } from "@/lib/utils"
import type { ParseHints } from "@/bindings/ParseHints"
import type { SnapshotPeriod } from "@/bindings/SnapshotPeriod"

interface Props {
  files: File[]
  onFilesChange: (files: File[]) => void
  hints: ParseHints
  onHintsChange: (hints: ParseHints) => void
  onSubmit: () => void
  onSkip?: () => void
  submitting?: boolean
  accountName: string
  accountInstitution: string
}

const PERIOD_LABELS: Record<"none" | SnapshotPeriod, string> = {
  none: "Only snapshots present in the document",
  monthly: "Monthly snapshots",
  quarterly: "Quarterly snapshots",
  yearly: "Yearly snapshots",
}

// The parser returns a bounded number of rows per pass (~150-250), so very large
// documents can truncate or misread. Warn before upload so the user can split.
const CSV_ROW_WARN = 200
const PDF_SIZE_WARN = 1.5 * 1024 * 1024
const LARGE_FILE_WARN = 3 * 1024 * 1024
const fmtMb = (bytes: number) => `${(bytes / 1024 / 1024).toFixed(1)} MB`

export function FileUpload({ files, onFilesChange, hints, onHintsChange, onSubmit, onSkip, submitting, accountName, accountInstitution }: Props) {
  const [dragOver, setDragOver] = useState(false)
  const [showContext, setShowContext] = useState(() => !!hints.hint)
  const [sizeWarnings, setSizeWarnings] = useState<string[]>([])
  const inputRef = useRef<HTMLInputElement>(null)

  // Flag files likely to exceed the parser's per-pass row budget. CSV/TSV are
  // measured by row count (read client-side); PDFs and other files by size.
  useEffect(() => {
    let cancelled = false
    async function check() {
      const warns: string[] = []
      for (const f of files) {
        const lower = f.name.toLowerCase()
        if (lower.endsWith(".csv") || lower.endsWith(".tsv")) {
          try {
            const text = await f.text()
            const rows = text.split(/\r?\n/).filter((l) => l.trim() !== "").length - 1
            if (rows > CSV_ROW_WARN) warns.push(`${f.name} — ~${rows.toLocaleString()} rows`)
          } catch {
            if (f.size > LARGE_FILE_WARN) warns.push(`${f.name} — ${fmtMb(f.size)}`)
          }
        } else if (lower.endsWith(".pdf")) {
          if (f.size > PDF_SIZE_WARN) warns.push(`${f.name} — ${fmtMb(f.size)} PDF`)
        } else if (f.size > LARGE_FILE_WARN) {
          warns.push(`${f.name} — ${fmtMb(f.size)}`)
        }
      }
      if (!cancelled) setSizeWarnings(warns)
    }
    check()
    return () => {
      cancelled = true
    }
  }, [files])

  function addFiles(newFiles: FileList | null) {
    if (!newFiles) return
    const arr = Array.from(newFiles)
    // Deduplicate by name+size
    const existing = new Set(files.map((f) => `${f.name}-${f.size}`))
    const unique = arr.filter((f) => !existing.has(`${f.name}-${f.size}`))
    onFilesChange([...files, ...unique])
  }

  function removeFile(index: number) {
    onFilesChange(files.filter((_, i) => i !== index))
  }

  function handleDrop(e: DragEvent) {
    e.preventDefault()
    setDragOver(false)
    addFiles(e.dataTransfer.files)
  }

  return (
    <div className="space-y-4">
      <div>
        <h2 className="text-lg font-semibold">{accountName}</h2>
        <p className="text-sm text-muted-foreground">{accountInstitution}</p>
      </div>

      {/* Drop zone */}
      <div
        className={cn(
          "border-2 border-dashed rounded-lg p-8 text-center transition-colors cursor-pointer",
          dragOver ? "border-primary bg-primary/5" : "border-muted-foreground/25 hover:border-muted-foreground/50"
        )}
        onDragOver={(e) => { e.preventDefault(); setDragOver(true) }}
        onDragLeave={() => setDragOver(false)}
        onDrop={handleDrop}
        onClick={() => inputRef.current?.click()}
      >
        <Upload className="h-8 w-8 mx-auto text-muted-foreground mb-3" />
        <p className="text-sm font-medium">
          Drop files here or click to browse
        </p>
        <p className="text-xs text-muted-foreground mt-1">
          CSV, PDF, and image files accepted
        </p>
        <input
          ref={inputRef}
          type="file"
          multiple
          accept=".csv,.pdf,.png,.jpg,.jpeg,.webp,.gif"
          className="hidden"
          onChange={(e) => addFiles(e.target.files)}
        />
      </div>

      {/* File list */}
      {files.length > 0 && (
        <div className="space-y-1">
          {files.map((file, idx) => (
            <div key={`${file.name}-${idx}`} className="flex items-center gap-2 rounded-lg border p-2">
              <FileText className="h-4 w-4 text-muted-foreground shrink-0" />
              <div className="flex-1 min-w-0">
                <p className="text-sm truncate">{file.name}</p>
                <p className="text-xs text-muted-foreground">{(file.size / 1024).toFixed(1)} KB</p>
              </div>
              <Button variant="ghost" size="icon" className="h-7 w-7 shrink-0" onClick={() => removeFile(idx)}>
                <X className="h-3.5 w-3.5" />
              </Button>
            </div>
          ))}
        </div>
      )}

      {/* Large-document warning */}
      {sizeWarnings.length > 0 && (
        <div className="flex gap-2 rounded-lg border border-amber-500/40 bg-amber-50 p-3 text-sm dark:bg-amber-950/30">
          <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0 text-amber-600" />
          <div className="text-amber-800 dark:text-amber-200">
            <p className="font-medium">
              Large document{sizeWarnings.length > 1 ? "s" : ""} — may not parse fully
            </p>
            <p className="mt-1 text-xs">
              The parser returns a limited number of rows per pass, so very large files can be
              truncated or misread. For best results, split into smaller chunks (a few months, or
              under ~200 rows, per file) before importing.
            </p>
            <ul className="mt-1 list-disc pl-4 text-xs">
              {sizeWarnings.map((w, i) => (
                <li key={i}>{w}</li>
              ))}
            </ul>
          </div>
        </div>
      )}

      {/* What does this document contain? */}
      <div className="space-y-3 rounded-lg border p-3">
        <p className="text-sm font-medium">What does this document contain?</p>
        <div className="flex flex-wrap items-center gap-x-6 gap-y-3">
          <label className="flex items-center gap-2 cursor-pointer text-sm">
            <Checkbox
              checked={hints.return_type.transactions}
              onCheckedChange={(v) =>
                onHintsChange({
                  ...hints,
                  return_type: { ...hints.return_type, transactions: !!v },
                })
              }
              disabled={submitting}
            />
            Transactions
          </label>
          <label className="flex items-center gap-2 cursor-pointer text-sm">
            <Checkbox
              checked={hints.return_type.holdings.enabled}
              onCheckedChange={(v) =>
                onHintsChange({
                  ...hints,
                  return_type: {
                    ...hints.return_type,
                    holdings: { ...hints.return_type.holdings, enabled: !!v },
                  },
                })
              }
              disabled={submitting}
            />
            Holdings
          </label>
          <label className="flex items-center gap-2 cursor-pointer text-sm">
            <Checkbox
              checked={hints.return_type.investments}
              onCheckedChange={(v) =>
                onHintsChange({
                  ...hints,
                  return_type: { ...hints.return_type, investments: !!v },
                })
              }
              disabled={submitting}
            />
            Investments
          </label>
        </div>
        {hints.return_type.holdings.enabled && (
          <div className="flex items-center gap-3 pt-1">
            <span className="text-xs text-muted-foreground">Snapshot frequency:</span>
            <Select
              value={hints.return_type.holdings.period ?? "none"}
              onValueChange={(v) =>
                onHintsChange({
                  ...hints,
                  return_type: {
                    ...hints.return_type,
                    holdings: {
                      ...hints.return_type.holdings,
                      period: v === "none" ? null : (v as SnapshotPeriod),
                    },
                  },
                })
              }
              disabled={submitting}
              items={PERIOD_LABELS}
            >
              <SelectTrigger className="h-8 w-auto min-w-[16rem] text-xs">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {(Object.entries(PERIOD_LABELS) as Array<[
                  "none" | SnapshotPeriod,
                  string,
                ]>).map(([key, label]) => (
                  <SelectItem key={key} value={key}>{label}</SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        )}
      </div>

      {/* Optional free-text context passed to the parsing agent via hints.hint */}
      <div>
        <button
          type="button"
          onClick={() => setShowContext((v) => !v)}
          disabled={submitting}
          className="flex items-center gap-1.5 text-sm text-muted-foreground hover:text-foreground disabled:opacity-50"
          aria-expanded={showContext}
        >
          <ChevronRight className={cn("h-4 w-4 transition-transform", showContext && "rotate-90")} />
          Add additional context (optional)
        </button>
        {showContext && (
          <div className="pt-2">
            <Textarea
              value={hints.hint ?? ""}
              onChange={(e) => {
                const v = e.target.value
                onHintsChange({ ...hints, hint: v.trim() === "" ? null : v })
              }}
              disabled={submitting}
              rows={3}
              placeholder="Anything that helps the agent read this document correctly, e.g. &quot;amounts are in EUR&quot;, &quot;ignore the summary page&quot;, or &quot;this is a joint account&quot;."
            />
            <p className="pt-1.5 text-xs text-muted-foreground">
              Only sent when filled in. Passed to the AI as extra context for this import.
            </p>
          </div>
        )}
      </div>

      {/* Actions */}
      <div className="flex justify-between">
        {onSkip && (
          <Button variant="outline" onClick={onSkip} disabled={submitting}>
            Skip account
          </Button>
        )}
        <div className="flex gap-2 ml-auto">
          <Button
            onClick={onSubmit}
            disabled={
              files.length === 0 ||
              submitting ||
              (!hints.return_type.transactions &&
                !hints.return_type.holdings.enabled &&
                !hints.return_type.investments)
            }
            className="bg-blue-600 text-white hover:bg-blue-600/90"
          >
            {submitting ? "Importing..." : "Import"}
          </Button>
        </div>
      </div>
    </div>
  )
}
