import { Badge } from "@/components/ui/badge"
import type { IngestionMetadata } from "@/bindings/IngestionMetadata"
import { CostTag } from "./cost_tag"

export function MetadataHeader({ metadata, fileCount }: { metadata: IngestionMetadata; fileCount: number }) {
  const confidence = Math.round(metadata.detection_confidence * 100)
  const ms = Number(metadata.processing_time_ms)
  // Backend skips empty arrays via `#[serde(skip_serializing_if = "Vec::is_empty")]`,
  // so these fields may arrive as `undefined` despite the binding declaring them
  // non-optional. Guard at the consumer.
  const notes = metadata.notes ?? []
  const relationshipsFound = metadata.relationships_found ?? []
  return (
    <div className="space-y-2">
      <div className="flex flex-wrap items-center gap-2">
        {metadata.institution_detected && (
          <Badge variant="outline" className="capitalize">
            {metadata.institution_detected}
          </Badge>
        )}
        <span className="text-xs text-muted-foreground">
          {fileCount} file{fileCount !== 1 ? "s" : ""} parsed
        </span>
        <span className="text-xs text-muted-foreground">·</span>
        <span className="text-xs text-muted-foreground">
          Detection confidence: <span className="tabular-nums">{confidence}%</span>
        </span>
        {ms > 0 && (
          <>
            <span className="text-xs text-muted-foreground">·</span>
            <span className="text-xs text-muted-foreground tabular-nums">{(ms / 1000).toFixed(1)}s</span>
          </>
        )}
        <CostTag price={metadata.estimated_price} className="ml-auto" />
      </div>

      {notes.length > 0 && (
        <ul className="space-y-1">
          {notes.map((note, i) => (
            <li key={i} className="text-xs text-muted-foreground">· {note}</li>
          ))}
        </ul>
      )}

      {relationshipsFound.length > 0 && (
        <ul className="space-y-1">
          {relationshipsFound.map((r, i) => (
            <li key={i} className="text-xs text-muted-foreground">⇄ {r}</li>
          ))}
        </ul>
      )}
    </div>
  )
}
