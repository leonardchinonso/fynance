import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { formatDate } from "@/lib/utils"

export interface SourceDocMeta {
  filename: string
  uploaded_at?: string
}

/**
 * Renders a row's source documents as a compact numbered list of chips
 * (`1 2 3`). Each chip is hoverable; the tooltip shows the document's filename
 * and upload date, resolved from `docs`. Empty list renders a muted dash.
 *
 * Used both in the import preview (docs come from `IngestionPreview.documents`)
 * and on the data tables (docs come from `GET /api/documents`).
 */
export function SourceChips({
  documentIds,
  docs,
}: {
  documentIds: string[] | undefined | null
  docs: Map<string, SourceDocMeta>
}) {
  if (!documentIds || documentIds.length === 0) {
    return <span className="text-xs text-muted-foreground">—</span>
  }
  return (
    <span className="inline-flex items-center gap-1.5">
      {documentIds.map((id, i) => {
        const meta = docs.get(id)
        return (
          <Tooltip key={`${id}-${i}`}>
            <TooltipTrigger
              render={
                <span className="text-xs tabular-nums cursor-help underline decoration-dotted decoration-muted-foreground/70 underline-offset-2 hover:text-foreground">
                  {i + 1}
                </span>
              }
            />
            <TooltipContent>
              {meta ? (
                <div className="space-y-0.5">
                  <div className="font-medium">{meta.filename}</div>
                  {meta.uploaded_at && (
                    <div className="text-xs text-muted-foreground">
                      Uploaded {formatDate(meta.uploaded_at)}
                    </div>
                  )}
                </div>
              ) : (
                <div className="text-xs">Source document {id.slice(0, 8)}…</div>
              )}
            </TooltipContent>
          </Tooltip>
        )
      })}
    </span>
  )
}
