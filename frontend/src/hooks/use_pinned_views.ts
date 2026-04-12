import { useState, useCallback } from "react"

interface PinnedView {
  id: string
  label: string
  url: string
  createdAt: string
}

const STORAGE_KEY = "fynance-pinned-views"

function loadPinned(): PinnedView[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return []
    const views = JSON.parse(raw) as PinnedView[]
    // Backfill IDs for views saved before ID was added
    let needsSave = false
    for (const v of views) {
      if (!v.id) {
        v.id = `pin_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`
        needsSave = true
      }
    }
    if (needsSave) savePinned(views)
    return views
  } catch {
    return []
  }
}

function savePinned(views: PinnedView[]) {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(views))
}

export function usePinnedViews() {
  const [pinnedViews, setPinnedViews] = useState<PinnedView[]>(loadPinned)

  const pinCurrentView = useCallback(
    (label: string) => {
      const url = window.location.pathname + window.location.search
      const id = `pin_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`
      const next = [
        ...pinnedViews.filter((v) => v.url !== url),
        { id, label, url, createdAt: new Date().toISOString() },
      ]
      setPinnedViews(next)
      savePinned(next)
    },
    [pinnedViews]
  )

  const unpinView = useCallback(
    (url: string) => {
      const next = pinnedViews.filter((v) => v.url !== url)
      setPinnedViews(next)
      savePinned(next)
    },
    [pinnedViews]
  )

  const renamePinnedView = useCallback(
    (url: string, newLabel: string) => {
      const next = pinnedViews.map((v) =>
        v.url === url ? { ...v, label: newLabel } : v
      )
      setPinnedViews(next)
      savePinned(next)
    },
    [pinnedViews]
  )

  const reorderPinnedViews = useCallback(
    (fromIndex: number, toIndex: number) => {
      setPinnedViews((prev) => {
        const next = [...prev]
        const [moved] = next.splice(fromIndex, 1)
        next.splice(toIndex, 0, moved)
        savePinned(next)
        return next
      })
    },
    []
  )

  return { pinnedViews, pinCurrentView, unpinView, renamePinnedView, reorderPinnedViews }
}
