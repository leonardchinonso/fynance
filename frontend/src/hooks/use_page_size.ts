import { useCallback } from "react"
import { useSearchParams } from "react-router-dom"

export const PAGE_SIZE_OPTIONS = [10, 25, 50, 100]
const DEFAULT_PAGE_SIZE = 25

/**
 * Page size stored solely in a URL query param — the URL is the single source of
 * truth (no localStorage), so an empty URL is always the default 25 and the
 * value round-trips in shared/pinned links, exactly like the other filters. The
 * param is omitted at the default to keep URLs clean.
 *
 * Pass `pageKey` to also reset that page param to 1 (by deleting it) when the
 * size changes, in a single history entry, for tables whose page lives in the URL.
 */
export function usePageSizeParam(
  sizeKey: string,
  pageKey?: string,
): [number, (size: number) => void] {
  const [searchParams, setSearchParams] = useSearchParams()
  const fromUrl = parseInt(searchParams.get(sizeKey) ?? "", 10)
  const pageSize = PAGE_SIZE_OPTIONS.includes(fromUrl) ? fromUrl : DEFAULT_PAGE_SIZE

  const setPageSize = useCallback(
    (size: number) => {
      setSearchParams((prev) => {
        const next = new URLSearchParams(prev)
        if (size === DEFAULT_PAGE_SIZE) next.delete(sizeKey)
        else next.set(sizeKey, String(size))
        if (pageKey) next.delete(pageKey)
        return next
      })
    },
    [sizeKey, pageKey, setSearchParams],
  )

  return [pageSize, setPageSize]
}
