import { useState, useRef, type DragEvent } from "react"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Upload, X, FileText } from "lucide-react"
import { cn } from "@/lib/utils"
import type { ParseHints } from "@/bindings/ParseHints"
import type { SnapshotPeriod } from "@/bindings/SnapshotPeriod"
import type { ParseMode } from "@/bindings/ParseMode"
import type { Agent } from "@/bindings/Agent"

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

export function FileUpload({ files, onFilesChange, hints, onHintsChange, onSubmit, onSkip, submitting, accountName, accountInstitution }: Props) {
  const [dragOver, setDragOver] = useState(false)
  const inputRef = useRef<HTMLInputElement>(null)

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
          CSV, PDF files accepted
        </p>
        <input
          ref={inputRef}
          type="file"
          multiple
          accept=".csv,.pdf"
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

      {/* EXPERIMENTAL knobs. */}
      <div className="space-y-2 rounded-lg border border-dashed border-muted-foreground/30 p-3">
        <div className="flex items-center gap-2">
          <p className="text-sm font-medium">Parsing strategy</p>
          <span className="rounded-full border border-amber-500/30 bg-amber-500/10 px-1.5 py-0.5 text-[10px] font-medium text-amber-600">
            experimental
          </span>
        </div>
        <p className="text-[11px] text-muted-foreground">
          Try different parsing strategies and model agents. May be removed
          once one is promoted to default.
        </p>
        <div className="flex flex-wrap items-center gap-x-6 gap-y-3 pt-1">
          <label className="flex items-center gap-2 text-sm">
            <span className="text-xs text-muted-foreground">Mode:</span>
            <Select
              value={hints.experimental?.mode ?? "split"}
              onValueChange={(v) =>
                onHintsChange({
                  ...hints,
                  experimental: {
                    mode: v as ParseMode,
                    agent: hints.experimental?.agent ?? null,
                  },
                })
              }
              disabled={submitting}
            >
              <SelectTrigger className="h-8 w-auto min-w-[10rem] text-xs">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="split">Split (per-type)</SelectItem>
                <SelectItem value="unified">Unified (single call)</SelectItem>
              </SelectContent>
            </Select>
          </label>
          <label className="flex items-center gap-2 text-sm">
            <span className="text-xs text-muted-foreground">Agent:</span>
            <Select
              value={hints.experimental?.agent ?? "default"}
              onValueChange={(v) =>
                onHintsChange({
                  ...hints,
                  experimental: {
                    mode: hints.experimental?.mode ?? "split",
                    agent: v === "default" ? null : (v as Agent),
                  },
                })
              }
              disabled={submitting}
            >
              <SelectTrigger className="h-8 w-auto min-w-[8rem] text-xs">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="default">Default</SelectItem>
                <SelectItem value="haiku">Haiku</SelectItem>
                <SelectItem value="sonnet">Sonnet</SelectItem>
                <SelectItem value="opus">Opus</SelectItem>
              </SelectContent>
            </Select>
          </label>
        </div>
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
