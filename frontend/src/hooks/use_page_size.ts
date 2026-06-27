import { useCallback } from "react"
import { useSearchParams } from "react-router-dom"

export const PAGE_SIZE_OPTIONS = [10, 25, 50, 100]
const PAGE_SIZE_KEY = "fynance-page-size"
const DEFAULT_PAGE_SIZE = 25

function rememberedDefault(): number {
  try {
    const n = parseInt(localStorage.getItem(PAGE_SIZE_KEY) ?? "", 10)
    return PAGE_SIZE_OPTIONS.includes(n) ? n : DEFAULT_PAGE_SIZE
  } catch {
    return DEFAULT_PAGE_SIZE
  }
}

/**
 * Page size synced to a URL query param so it round-trips in shared and pinned
 * links, falling back to the per-browser remembered default (then 25) when the
 * param is absent. The param is omitted at the default to keep URLs clean.
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
  const pageSize = PAGE_SIZE_OPTIONS.includes(fromUrl) ? fromUrl : rememberedDefault()

  const setPageSize = useCallback(
    (size: number) => {
      try { localStorage.setItem(PAGE_SIZE_KEY, String(size)) } catch { /* ignore */ }
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
