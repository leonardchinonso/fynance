import { useSyncExternalStore } from "react"
import { X } from "lucide-react"

interface Toast {
  id: number
  message: string
}

const MAX_TOASTS = 3
const AUTO_DISMISS_MS = 6000

// Module-scope store (same pattern as the redacted flag in lib/utils.ts) so
// showErrorToast is callable from any async handler without React context.
let toasts: Toast[] = []
let nextId = 1
const listeners = new Set<() => void>()

function emit() {
  for (const cb of listeners) cb()
}

function subscribe(cb: () => void): () => void {
  listeners.add(cb)
  return () => {
    listeners.delete(cb)
  }
}

function getToasts(): Toast[] {
  return toasts
}

function dismissToast(id: number) {
  if (!toasts.some((t) => t.id === id)) return
  toasts = toasts.filter((t) => t.id !== id)
  emit()
}

/** Shows a transient error toast (bottom-right). Auto-dismisses after a few seconds. */
export function showErrorToast(message: string) {
  const id = nextId++
  toasts = [...toasts, { id, message }].slice(-MAX_TOASTS)
  emit()
  setTimeout(() => dismissToast(id), AUTO_DISMISS_MS)
}

/** Renders the toast stack. Mount once near the app root. */
export function Toaster() {
  const items = useSyncExternalStore(subscribe, getToasts)
  if (items.length === 0) return null
  return (
    <div className="fixed bottom-4 right-4 z-50 flex w-80 max-w-[calc(100vw-2rem)] flex-col gap-2">
      {items.map((t) => (
        <div
          key={t.id}
          role="alert"
          className="rounded-lg border border-destructive/30 bg-background shadow-lg animate-in fade-in-0 slide-in-from-bottom-2"
        >
          <div className="flex items-start gap-2 rounded-lg bg-destructive/5 p-3">
            <p className="flex-1 break-words text-sm text-destructive whitespace-pre-wrap">
              {t.message}
            </p>
            <button
              type="button"
              aria-label="Dismiss"
              className="shrink-0 rounded-md p-0.5 text-muted-foreground hover:text-foreground transition-colors"
              onClick={() => dismissToast(t.id)}
            >
              <X className="h-3.5 w-3.5" />
            </button>
          </div>
        </div>
      ))}
    </div>
  )
}
