import { api } from "@/api/client"
import type { DocumentSummary } from "@/bindings/DocumentSummary"
import type { RemoteData } from "@/lib/remote_data"
import { useQuery } from "@/hooks/use_query"

/**
 * Stored source documents (provenance list). Volatile: uploads, deletes and
 * parses invalidate it via the api client's mutation wrapper. Pass `enabled`
 * to gate the fetch (e.g. only while a Source column is visible).
 * `includeRefs` populates `reference_count` on every row; it is a soft dep so
 * the with-refs and without-refs shapes cache separately.
 */
export function useDocuments(enabled = true, includeRefs = false): [RemoteData<DocumentSummary[]>, () => void] {
  return useQuery(
    () => api.listDocuments(includeRefs),
    { tag: "documents", hard: [], soft: [includeRefs], enabled },
  )
}
