import { useState } from "react"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
} from "@/components/ui/select"
import { Button } from "@/components/ui/button"
import { ChevronLeft, ChevronRight } from "lucide-react"

const PAGE_SIZE_OPTIONS = [10, 25, 50, 100]

interface Props {
  title: string
  summary: React.ReactNode
  /** Section-specific filter controls (date, category, holding, etc). */
  filterSlot?: React.ReactNode
  totalRows: number
  pageSize: number
  page: number
  onPageChange: (p: number) => void
  onPageSizeChange: (s: number) => void
  children: React.ReactNode
}

export function SectionShell({
  title,
  summary,
  filterSlot,
  totalRows,
  pageSize,
  page,
  onPageChange,
  onPageSizeChange,
  children,
}: Props) {
  const totalPages = Math.max(1, Math.ceil(totalRows / pageSize))
  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div>
          <h3 className="text-base font-semibold">{title}</h3>
          <div className="text-xs text-muted-foreground">{summary}</div>
        </div>
        {filterSlot && <div className="flex flex-wrap items-center gap-2">{filterSlot}</div>}
      </div>

      <div className="overflow-x-auto rounded-lg border">{children}</div>

      <div className="flex flex-wrap items-center justify-between gap-2 px-1 text-xs text-muted-foreground">
        <div className="flex items-center gap-2">
          <span>{totalRows} row{totalRows !== 1 ? "s" : ""}</span>
          <span>·</span>
          <span>Show</span>
          <Select
            value={String(pageSize)}
            onValueChange={(v) => { if (v != null) onPageSizeChange(parseInt(v, 10)) }}
          >
            <SelectTrigger className="h-7 w-[60px] text-xs">
              <span>{pageSize}</span>
            </SelectTrigger>
            <SelectContent>
              {PAGE_SIZE_OPTIONS.map((s) => (
                <SelectItem key={s} value={String(s)}>
                  {s}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <div className="flex items-center gap-2">
          <span>
            Page {page} of {totalPages}
          </span>
          <div className="flex gap-1">
            <Button
              variant="outline"
              size="sm"
              className="h-7 w-7 p-0"
              disabled={page <= 1}
              onClick={() => onPageChange(page - 1)}
            >
              <ChevronLeft className="h-3.5 w-3.5" />
            </Button>
            <Button
              variant="outline"
              size="sm"
              className="h-7 w-7 p-0"
              disabled={page >= totalPages}
              onClick={() => onPageChange(page + 1)}
            >
              <ChevronRight className="h-3.5 w-3.5" />
            </Button>
          </div>
        </div>
      </div>
    </div>
  )
}

export function useSectionControls() {
  const [page, setPage] = useState(1)
  const [pageSize, setPageSize] = useState(25)
  return { page, setPage, pageSize, setPageSize }
}
