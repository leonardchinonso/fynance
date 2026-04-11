import { useState, useCallback } from "react"

interface PinnedView {
  label: string
  url: string
  createdAt: string
}

const STORAGE_KEY = "fynance-pinned-views"

function loadPinned(): PinnedView[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    return raw ? JSON.parse(raw) : []
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
      const next = [
        ...pinnedViews.filter((v) => v.url !== url),
        { label, url, createdAt: new Date().toISOString() },
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

  return { pinnedViews, pinCurrentView, unpinView }
}
