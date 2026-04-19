import { useRef, type ReactNode } from "react"
import { cn } from "@/lib/utils"

export interface DragHandleProps {
  "data-drag-handle": true
  className: string
  style: React.CSSProperties
}

export interface DraggableListProps<T extends { id: string }> {
  items: T[]
  onReorder: (fromIndex: number, toIndex: number) => void
  renderItem: (item: T, index: number) => ReactNode
  listClassName?: string
  itemClassName?: string
  /** ID of item currently being dragged (managed externally) */
  dragId: string | null
  onDragChange: (id: string | null) => void
}

export function DraggableList<T extends { id: string }>({
  items,
  onReorder,
  renderItem,
  listClassName,
  itemClassName,
  dragId,
  onDragChange,
}: DraggableListProps<T>) {
  const listRef = useRef<HTMLDivElement>(null)
  const rectsRef = useRef<Map<string, DOMRect>>(new Map())

  function captureRects() {
    if (!listRef.current) return
    const els = listRef.current.querySelectorAll("[data-drag-item]")
    const map = new Map<string, DOMRect>()
    els.forEach((el) => {
      const id = el.getAttribute("data-drag-id")
      if (id) map.set(id, el.getBoundingClientRect())
    })
    rectsRef.current = map
  }

  function animateRects() {
    if (!listRef.current) return
    const els = listRef.current.querySelectorAll("[data-drag-item]")
    els.forEach((el) => {
      const id = el.getAttribute("data-drag-id")
      if (!id) return
      const oldRect = rectsRef.current.get(id)
      if (!oldRect) return
      const newRect = el.getBoundingClientRect()
      const deltaY = oldRect.top - newRect.top
      if (Math.abs(deltaY) < 1) return
      const htmlEl = el as HTMLElement
      htmlEl.style.transition = "none"
      htmlEl.style.transform = `translateY(${deltaY}px)`
      requestAnimationFrame(() => {
        htmlEl.style.transition = "transform 200ms ease"
        htmlEl.style.transform = ""
      })
    })
  }

  return (
    <div ref={listRef} className={listClassName}>
      {items.map((item, idx) => (
        <div
          key={item.id}
          data-drag-item
          data-drag-id={item.id}
          className={cn(
            itemClassName,
            dragId === item.id && "invisible"
          )}
          style={{
            opacity: dragId !== null && dragId !== item.id ? 0.6 : 1,
            cursor: dragId === item.id ? "grabbing" : undefined,
          }}
          onPointerDown={(e) => {
            const target = e.target as HTMLElement
            if (!target.closest("[data-drag-handle]")) return
            e.preventDefault()
            const draggedId = item.id
            onDragChange(draggedId)
            let settling = false

            const sourceEl = e.currentTarget as HTMLElement
            const sourceRect = sourceEl.getBoundingClientRect()
            const ghost = sourceEl.cloneNode(true) as HTMLElement
            ghost.style.cssText = `
              position: fixed;
              left: ${sourceRect.left}px;
              top: ${sourceRect.top}px;
              width: ${sourceRect.width}px;
              pointer-events: none;
              z-index: 9999;
              opacity: 0.9;
              background: hsl(var(--muted));
              border: 1px solid hsl(var(--border));
              border-radius: 6px;
              box-shadow: 0 4px 12px rgba(0,0,0,0.3);
              transition: none;
            `
            document.body.appendChild(ghost)
            const offsetY = e.clientY - sourceRect.top

            const onMove = (ev: PointerEvent) => {
              if (listRef.current) {
                const listRect = listRef.current.getBoundingClientRect()
                const ghostHeight = sourceRect.height
                const minY = listRect.top
                const maxY = listRect.bottom - ghostHeight
                const targetY = Math.max(minY, Math.min(maxY, ev.clientY - offsetY))
                ghost.style.top = `${targetY}px`
              } else {
                ghost.style.top = `${ev.clientY - offsetY}px`
              }

              if (!listRef.current || settling) return

              const allItems = Array.from(listRef.current.querySelectorAll("[data-drag-item]"))
              const fromIdx = allItems.findIndex((el) => el.getAttribute("data-drag-id") === draggedId)
              if (fromIdx === -1) return

              let toIdx = -1
              let targetRect: DOMRect | null = null
              for (let i = 0; i < allItems.length; i++) {
                if (i === fromIdx) continue
                const rect = allItems[i].getBoundingClientRect()
                if (ev.clientY >= rect.top && ev.clientY <= rect.bottom) {
                  toIdx = i
                  targetRect = rect
                  break
                }
              }

              if (toIdx !== -1 && targetRect) {
                const threshold = targetRect.height * 0.25
                const isAbove = toIdx < fromIdx
                const inActiveZone = isAbove
                  ? ev.clientY <= targetRect.bottom - threshold
                  : ev.clientY >= targetRect.top + threshold
                if (!inActiveZone) toIdx = -1
              }

              if (toIdx === -1 || toIdx === fromIdx) return

              captureRects()
              onReorder(fromIdx, toIdx)

              settling = true
              requestAnimationFrame(() => requestAnimationFrame(() => {
                animateRects()
                setTimeout(() => { settling = false }, 200)
              }))
            }

            const onUp = () => {
              onDragChange(null)
              ghost.remove()
              document.removeEventListener("pointermove", onMove)
              document.removeEventListener("pointerup", onUp)
              document.body.style.cursor = ""
            }
            document.body.style.cursor = "grabbing"
            document.addEventListener("pointermove", onMove)
            document.addEventListener("pointerup", onUp)
          }}
        >
          {renderItem(item, idx)}
        </div>
      ))}
    </div>
  )
}

/** Standard drag handle icon (6-dot grip) */
export function DragHandle({ className }: { className?: string }) {
  return (
    <span
      data-drag-handle
      className={cn("cursor-grab active:cursor-grabbing p-1 text-muted-foreground opacity-30 group-hover:opacity-70 shrink-0 touch-none", className)}
      title="Drag to reorder"
    >
      <svg width="10" height="10" viewBox="0 0 10 10" fill="currentColor">
        <circle cx="3" cy="2" r="1" />
        <circle cx="7" cy="2" r="1" />
        <circle cx="3" cy="5" r="1" />
        <circle cx="7" cy="5" r="1" />
        <circle cx="3" cy="8" r="1" />
        <circle cx="7" cy="8" r="1" />
      </svg>
    </span>
  )
}
