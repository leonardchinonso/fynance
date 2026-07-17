import { useCallback, useEffect, useMemo, useRef, useState, type DragEvent } from "react"
import { useNavigate } from "react-router-dom"
import {
  ArrowLeft,
  Download,
  Trash2,
  Upload,
  AlertTriangle,
  FileWarning,
  Search,
  Check,
  ChevronsUpDown,
  ChevronLeft,
  ChevronRight,
  Settings2,
  ArrowUp,
  ArrowDown,
  ArrowUpDown,
} from "lucide-react"
import { api } from "@/api/client"
import { DocumentReferencedError } from "@/api/service"
import { useDocuments } from "@/hooks/data"
import type { DocumentSummary } from "@/bindings/DocumentSummary"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { Input } from "@/components/ui/input"
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
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
} from "@/components/ui/select"
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover"
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { TableSkeleton } from "@/components/skeletons"
import { usePageSizeParam, PAGE_SIZE_OPTIONS } from "@/hooks/use_page_size"
import { formatDate } from "@/lib/utils"

const COLUMNS_KEY = "fynance-doc-columns"

// Sentinel option for documents with no associated account.
const NO_ACCOUNT = "__none__"

// Stable empty list while the document query has no value yet.
const NO_DOCS: DocumentSummary[] = []

type DocSortColumn = "file" | "uploaded"
type SortDir = "asc" | "desc"

interface Column {
  id: string
  label: string
  defaultVisible: boolean
  align?: "right"
}

const ALL_COLUMNS: Column[] = [
  { id: "file", label: "File", defaultVisible: true },
  { id: "links", label: "Links", defaultVisible: false, align: "right" },
  { id: "type", label: "Type", defaultVisible: false },
  { id: "size", label: "Size", defaultVisible: false, align: "right" },
  { id: "origin", label: "Origin", defaultVisible: true },
  { id: "account", label: "Account", defaultVisible: true },
  { id: "uploaded", label: "Uploaded", defaultVisible: true },
]

function getStoredColumns(): Set<string> {
  try {
    const v = localStorage.getItem(COLUMNS_KEY)
    if (v) return new Set(JSON.parse(v))
  } catch { /* ignore */ }
  return new Set(ALL_COLUMNS.filter((c) => c.defaultVisible).map((c) => c.id))
}

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

function MultiSelect({
  label, options, selected, onChange, displayFn,
}: {
  label: string
  options: string[]
  selected: string[]
  onChange: (selected: string[]) => void
  displayFn?: (value: string) => string
}) {
  const [open, setOpen] = useState(false)
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger className="inline-flex shrink-0 items-center justify-center gap-1 rounded-md border bg-background px-3 py-1 text-sm font-medium shadow-xs hover:bg-accent hover:text-accent-foreground h-8">
        {label}
        {selected.length > 0 && <Badge variant="secondary" className="ml-1">{selected.length}</Badge>}
        <ChevronsUpDown className="ml-1 h-3 w-3 opacity-50" />
      </PopoverTrigger>
      <PopoverContent className="w-[250px] p-0" align="start">
        <Command>
          <CommandInput placeholder={`Search ${label.toLowerCase()}...`} />
          <CommandList>
            <CommandEmpty>No results.</CommandEmpty>
            <CommandGroup>
              {options.map((opt) => (
                <CommandItem
                  key={opt}
                  value={`${displayFn ? displayFn(opt) : opt} ${opt}`}
                  onSelect={() => onChange(
                    selected.includes(opt) ? selected.filter(s => s !== opt) : [...selected, opt]
                  )}
                >
                  <Check className={cn("mr-2 h-4 w-4", selected.includes(opt) ? "opacity-100" : "opacity-0")} />
                  {displayFn ? displayFn(opt) : opt}
                </CommandItem>
              ))}
            </CommandGroup>
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  )
}

function SortableHead({
  label, column, active, dir, onClick, className,
}: {
  label: string
  column: DocSortColumn
  active: DocSortColumn
  dir: SortDir
  onClick: (col: DocSortColumn) => void
  className?: string
}) {
  const isActive = active === column
  const Icon = !isActive ? ArrowUpDown : dir === "asc" ? ArrowUp : ArrowDown
  return (
    <TableHead className={className}>
      <button
        type="button"
        onClick={() => onClick(column)}
        className={cn(
          "inline-flex items-center gap-1 select-none cursor-pointer rounded-md px-1 py-0.5 -mx-1 hover:bg-muted transition-colors",
          isActive ? "text-foreground" : "text-muted-foreground hover:text-foreground",
        )}
        aria-label={`Sort by ${label}`}
      >
        <span>{label}</span>
        <Icon className={cn("h-3 w-3", isActive ? "opacity-100" : "opacity-50")} />
      </button>
    </TableHead>
  )
}

function ColumnSettings({
  columns,
  visible,
  onToggle,
}: {
  columns: Column[]
  visible: Set<string>
  onToggle: (id: string) => void
}) {
  return (
    <Popover>
      <PopoverTrigger className="inline-flex items-center justify-center rounded-md p-1 hover:bg-muted transition-colors">
        <Settings2 className="h-4 w-4 text-muted-foreground" />
      </PopoverTrigger>
      <PopoverContent className="w-[180px] p-2" align="end">
        <p className="text-xs font-medium text-muted-foreground mb-2">Visible columns</p>
        {columns.map((col) => (
          <button
            key={col.id}
            onClick={() => onToggle(col.id)}
            className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-sm hover:bg-muted transition-colors"
          >
            <Check className={`h-3.5 w-3.5 ${visible.has(col.id) ? "opacity-100" : "opacity-0"}`} />
            {col.label}
          </button>
        ))}
      </PopoverContent>
    </Popover>
  )
}

/**
 * Resolves the per-document reference count lazily and non-blockingly:
 * orphaned docs short-circuit to 0 with no fetch; a known number is shown
 * directly; otherwise the count is fetched once (cached in the page-level
 * `counts` map so paging back never refetches) while a small skeleton shows.
 */
function LinksCell({
  doc,
  counts,
  onResolved,
}: {
  doc: DocumentSummary
  counts: Map<string, number>
  onResolved: (id: string, count: number) => void
}) {
  const known = doc.orphaned
    ? 0
    : typeof doc.reference_count === "number"
      ? doc.reference_count
      : counts.get(doc.id)

  const [value, setValue] = useState<number | undefined>(known)

  useEffect(() => {
    if (known !== undefined) {
      setValue(known)
      return
    }
    let cancelled = false
    api
      .getDocument(doc.id)
      .then((d) => {
        if (cancelled) return
        const count = d.orphaned ? 0 : (d.reference_count ?? 0)
        onResolved(doc.id, count)
        setValue(count)
      })
      .catch(() => { if (!cancelled) setValue(0) })
    return () => { cancelled = true }
  }, [doc.id, known, onResolved])

  if (value === undefined) {
    return <span className="inline-block h-3 w-6 animate-pulse rounded bg-muted align-middle ml-auto" />
  }
  return <span className="tabular-nums">{value}</span>
}

export function DocumentsPage() {
  const navigate = useNavigate()
  // Shared cached list: uploads/deletes invalidate it via the api client's
  // mutation wrapper, which force-refetches this active query. Refs are
  // included so the Links column renders without per-row lookups.
  const [docsData, refreshDocs] = useDocuments(true, true)
  const docs =
    docsData.status === "succeeded" || docsData.status === "reloading" ? docsData.value : NO_DOCS
  const loading = docsData.status === "loading" || docsData.status === "idle"
  const listError = docsData.status === "failed" ? docsData.error : null
  // Upload/delete failures; list failures come from the query above.
  const [error, setError] = useState<string | null>(null)
  const [uploading, setUploading] = useState(false)
  const [dragOver, setDragOver] = useState(false)
  const fileInputRef = useRef<HTMLInputElement>(null)

  // id -> display name for the Account column / filter.
  const [accountNames, setAccountNames] = useState<Record<string, string>>({})

  // Lazily-resolved per-document reference counts, lifted here so paging back
  // and forth doesn't refetch. Keyed by document id.
  const [counts] = useState<Map<string, number>>(() => new Map())

  // Filters (all client-side; the list is cheap now).
  const [orphanFilter, setOrphanFilter] = useState<"all" | "orphaned" | "linked">("all")
  const [selectedAccounts, setSelectedAccounts] = useState<string[]>([])
  const [search, setSearch] = useState("")

  // Table view state.
  const [visibleColumns, setVisibleColumns] = useState<Set<string>>(getStoredColumns)
  const [pageSize, setPageSize] = usePageSizeParam("doc_limit")
  const [page, setPage] = useState(1)
  const [sort, setSort] = useState<DocSortColumn>("uploaded")
  const [sortDir, setSortDir] = useState<SortDir>("desc")

  // The document pending a force-delete confirmation, plus its reference breakdown.
  const [confirm, setConfirm] = useState<{ doc: DocumentSummary; references: References } | null>(null)
  const [deleting, setDeleting] = useState(false)

  useEffect(() => {
    let cancelled = false
    api
      .getAccounts()
      .then((accounts) => {
        if (cancelled) return
        setAccountNames(Object.fromEntries(accounts.map((a) => [a.id, a.name])))
      })
      .catch(() => { /* account names are optional context; fall back to raw ids */ })
    return () => { cancelled = true }
  }, [])

  const rememberCount = useCallback((id: string, count: number) => {
    counts.set(id, count)
  }, [counts])

  // Distinct account ids present in the docs, using a sentinel for null.
  const accountOptions = useMemo(() => {
    const ids = new Set<string>()
    for (const d of docs) ids.add(d.account_id ?? NO_ACCOUNT)
    return Array.from(ids).sort((a, b) => {
      if (a === NO_ACCOUNT) return 1
      if (b === NO_ACCOUNT) return -1
      return (accountNames[a] ?? a).localeCompare(accountNames[b] ?? b)
    })
  }, [docs, accountNames])

  const accountLabel = (id: string) =>
    id === NO_ACCOUNT ? "(none)" : (accountNames[id] ?? id)

  const filtered = useMemo(() => {
    const needle = search.trim().toLowerCase()
    return docs.filter((d) => {
      if (orphanFilter === "orphaned" && !d.orphaned) return false
      if (orphanFilter === "linked" && d.orphaned) return false
      if (selectedAccounts.length > 0 && !selectedAccounts.includes(d.account_id ?? NO_ACCOUNT)) return false
      if (needle && !d.filename.toLowerCase().includes(needle)) return false
      return true
    })
  }, [docs, orphanFilter, selectedAccounts, search])

  const hasFilters = orphanFilter !== "all" || selectedAccounts.length > 0 || search.length > 0

  const sorted = useMemo(() => {
    const dir = sortDir === "asc" ? 1 : -1
    return [...filtered].sort((a, b) => {
      const cmp = sort === "file"
        ? a.filename.localeCompare(b.filename)
        : a.uploaded_at.localeCompare(b.uploaded_at)
      // Stable tiebreak by upload time so equal keys keep a deterministic order.
      return (cmp || a.uploaded_at.localeCompare(b.uploaded_at)) * dir
    })
  }, [filtered, sort, sortDir])

  const total = sorted.length
  const totalPages = Math.max(1, Math.ceil(total / pageSize))
  const currentPage = Math.min(page, totalPages)
  const pageRows = sorted.slice((currentPage - 1) * pageSize, currentPage * pageSize)

  // Reset to the first page whenever the working set, sort, or page size changes.
  useEffect(() => { setPage(1) }, [orphanFilter, selectedAccounts, search, sort, sortDir, pageSize])

  function cycleSort(col: DocSortColumn) {
    if (sort === col) {
      setSortDir((d) => (d === "asc" ? "desc" : "asc"))
    } else {
      setSort(col)
      setSortDir(col === "uploaded" ? "desc" : "asc")
    }
  }

  function toggleColumn(colId: string) {
    setVisibleColumns((prev) => {
      const next = new Set(prev)
      if (next.has(colId)) next.delete(colId)
      else next.add(colId)
      localStorage.setItem(COLUMNS_KEY, JSON.stringify(Array.from(next)))
      return next
    })
  }

  const isVisible = (colId: string) => visibleColumns.has(colId)

  function clearFilters() {
    setOrphanFilter("all")
    setSelectedAccounts([])
    setSearch("")
  }

  async function handleUpload(files: FileList | null) {
    if (!files || files.length === 0) return
    setUploading(true)
    setError(null)
    try {
      await api.uploadDocuments(Array.from(files))
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
    } catch (e: unknown) {
      if (e instanceof DocumentReferencedError) {
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

      {(error ?? listError) && (
        <div className="mb-4 flex items-start gap-2 rounded-lg border border-destructive/40 bg-destructive/5 p-3 text-sm">
          <AlertTriangle className="h-4 w-4 text-destructive shrink-0 mt-0.5" />
          <span className="flex-1 text-xs text-destructive">{error ?? listError}</span>
          {!error && listError && (
            <Button variant="outline" size="sm" className="h-6 text-xs" onClick={refreshDocs}>
              Retry
            </Button>
          )}
        </div>
      )}

      {/* Filters */}
      <div className="mb-3 flex flex-wrap items-center gap-2">
        <Select
          value={orphanFilter}
          onValueChange={(v) => { if (v) setOrphanFilter(v as "all" | "orphaned" | "linked") }}
        >
          <SelectTrigger className="h-8 w-[150px] text-sm">
            <span>
              {orphanFilter === "all" ? "All" : orphanFilter === "orphaned" ? "Orphaned only" : "Linked only"}
            </span>
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">All</SelectItem>
            <SelectItem value="orphaned">Orphaned only</SelectItem>
            <SelectItem value="linked">Linked only</SelectItem>
          </SelectContent>
        </Select>

        <MultiSelect
          label="Accounts"
          options={accountOptions}
          selected={selectedAccounts}
          onChange={setSelectedAccounts}
          displayFn={accountLabel}
        />

        {hasFilters && (
          <Button variant="ghost" size="sm" onClick={clearFilters}>
            Clear filters
          </Button>
        )}

        <div className="flex-1" />

        <div className="relative">
          <Search className="absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
          <Input
            placeholder="Search documents..."
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            className="h-8 w-[200px] pl-8 text-sm"
          />
        </div>
      </div>

      {loading ? (
        <TableSkeleton rows={pageSize} cols={visibleColumns.size} bordered />
      ) : docs.length === 0 ? (
        <div className="rounded-lg border border-dashed p-10 text-center text-sm text-muted-foreground">
          No documents yet. Files you import appear here automatically, or upload one above.
        </div>
      ) : (
        <div className="rounded-xl border overflow-hidden">
          <Table>
            <TableHeader>
              <TableRow>
                {isVisible("file") && (
                  <SortableHead label="File" column="file" active={sort} dir={sortDir} onClick={cycleSort} />
                )}
                {isVisible("links") && <TableHead className="text-right">Links</TableHead>}
                {isVisible("type") && <TableHead>Type</TableHead>}
                {isVisible("size") && <TableHead className="text-right">Size</TableHead>}
                {isVisible("origin") && <TableHead>Origin</TableHead>}
                {isVisible("account") && <TableHead>Account</TableHead>}
                {isVisible("uploaded") && (
                  <SortableHead label="Uploaded" column="uploaded" active={sort} dir={sortDir} onClick={cycleSort} />
                )}
                <TableHead className="text-right">Actions</TableHead>
                <TableHead className="w-8">
                  <ColumnSettings columns={ALL_COLUMNS} visible={visibleColumns} onToggle={toggleColumn} />
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {pageRows.length === 0 ? (
                <TableRow>
                  <TableCell colSpan={9} className="text-center text-sm text-muted-foreground py-10">
                    No documents match the current filters.
                  </TableCell>
                </TableRow>
              ) : (
                pageRows.map((doc) => (
                  <TableRow key={doc.id}>
                    {isVisible("file") && (
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
                    )}
                    {isVisible("links") && (
                      <TableCell className="text-right tabular-nums text-xs">
                        <LinksCell doc={doc} counts={counts} onResolved={rememberCount} />
                      </TableCell>
                    )}
                    {isVisible("type") && (
                      <TableCell className="text-xs text-muted-foreground">{doc.mime_type}</TableCell>
                    )}
                    {isVisible("size") && (
                      <TableCell className="text-right tabular-nums text-xs">{formatBytes(doc.size_bytes)}</TableCell>
                    )}
                    {isVisible("origin") && (
                      <TableCell>
                        <Badge variant="secondary" className="text-xs">{doc.origin}</Badge>
                      </TableCell>
                    )}
                    {isVisible("account") && (
                      <TableCell className="text-xs text-muted-foreground">
                        {doc.account_id ? (accountNames[doc.account_id] ?? doc.account_id) : "—"}
                      </TableCell>
                    )}
                    {isVisible("uploaded") && (
                      <TableCell className="text-xs text-muted-foreground whitespace-nowrap">
                        {formatDate(doc.uploaded_at)}
                      </TableCell>
                    )}
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
                    <TableCell />
                  </TableRow>
                ))
              )}
            </TableBody>
          </Table>

          {/* Pagination */}
          <div className="flex items-center justify-between border-t px-2 py-3">
            <div className="flex items-center gap-3">
              <span className="text-sm text-muted-foreground">
                {total} document{total === 1 ? "" : "s"}
              </span>
              <div className="flex items-center gap-1.5">
                <span className="text-xs text-muted-foreground">Show</span>
                <Select
                  value={pageSize.toString()}
                  onValueChange={(v) => {
                    if (v == null) return
                    setPageSize(parseInt(v, 10))
                  }}
                >
                  <SelectTrigger className="h-7 w-[65px] text-xs">
                    <span>{pageSize}</span>
                  </SelectTrigger>
                  <SelectContent>
                    {PAGE_SIZE_OPTIONS.map((size) => (
                      <SelectItem key={size} value={size.toString()}>
                        {size}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                <span className="text-xs text-muted-foreground">per page</span>
              </div>
            </div>
            <div className="flex items-center gap-2">
              <span className="text-sm text-muted-foreground">
                Page {currentPage} of {totalPages}
              </span>
              <div className="flex gap-1">
                <Button
                  variant="outline"
                  size="sm"
                  className="h-7 w-7 p-0"
                  disabled={currentPage <= 1}
                  onClick={() => setPage(currentPage - 1)}
                >
                  <ChevronLeft className="h-4 w-4" />
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  className="h-7 w-7 p-0"
                  disabled={currentPage >= totalPages}
                  onClick={() => setPage(currentPage + 1)}
                >
                  <ChevronRight className="h-4 w-4" />
                </Button>
              </div>
            </div>
          </div>
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
