import { useState, useCallback, useEffect, useRef } from "react"
import { createPortal } from "react-dom"

export interface ContextMenuItem {
  label: string
  onSelect: () => void
}

interface MenuState {
  x: number
  y: number
  items: ContextMenuItem[]
}

/**
 * Cursor-anchored right-click menu for charts. `open(event, items)` captures the
 * pointer position and shows the menu; it closes on select, outside-click,
 * Escape, or scroll. Pair the returned `menu`/`close` with `<ChartContextMenu>`.
 */
export function useChartContextMenu() {
  const [menu, setMenu] = useState<MenuState | null>(null)

  const open = useCallback(
    (e: { clientX: number; clientY: number; preventDefault: () => void }, items: ContextMenuItem[]) => {
      e.preventDefault()
      if (items.length === 0) return
      setMenu({ x: e.clientX, y: e.clientY, items })
    },
    [],
  )

  const close = useCallback(() => setMenu(null), [])

  return { menu, open, close }
}

const MENU_WIDTH = 240
const ITEM_HEIGHT = 34

export function ChartContextMenu({ menu, onClose }: { menu: MenuState | null; onClose: () => void }) {
  const ref = useRef<HTMLDivElement>(null)
  useEffect(() => {
    if (!menu) return
    // Ignore pointerdowns inside the menu so a click on an item isn't swallowed
    // by the outside-click close (which would unmount the item before its click).
    const onDown = (e: PointerEvent) => {
      if (ref.current && ref.current.contains(e.target as Node)) return
      onClose()
    }
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") onClose() }
    const onScroll = () => onClose()
    // Defer attaching so the same right-click that opened the menu doesn't close it.
    const id = window.setTimeout(() => {
      window.addEventListener("pointerdown", onDown)
      window.addEventListener("keydown", onKey)
      window.addEventListener("scroll", onScroll, true)
    }, 0)
    return () => {
      window.clearTimeout(id)
      window.removeEventListener("pointerdown", onDown)
      window.removeEventListener("keydown", onKey)
      window.removeEventListener("scroll", onScroll, true)
    }
  }, [menu, onClose])

  if (!menu) return null

  // Clamp to the viewport so the menu never spills off-screen.
  const left = Math.min(menu.x, window.innerWidth - MENU_WIDTH - 8)
  const maxTop = window.innerHeight - menu.items.length * ITEM_HEIGHT - 8
  const top = Math.min(menu.y, Math.max(8, maxTop))

  return createPortal(
    <div
      ref={ref}
      className="fixed z-[100] min-w-[12rem] overflow-hidden rounded-md border border-border/50 bg-popover p-1 text-popover-foreground shadow-md"
      style={{ left, top, width: MENU_WIDTH }}
      onContextMenu={(e) => e.preventDefault()}
    >
      {menu.items.map((item, i) => (
        <button
          key={i}
          type="button"
          className="flex w-full items-center rounded-sm px-2 py-1.5 text-left text-sm hover:bg-accent hover:text-accent-foreground"
          onClick={() => { item.onSelect(); onClose() }}
        >
          {item.label}
        </button>
      ))}
    </div>,
    document.body,
  )
}
